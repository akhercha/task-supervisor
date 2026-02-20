pub(crate) mod builder;
pub(crate) mod handle;

use std::{
    collections::{BinaryHeap, HashMap},
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::{sync::mpsc, time::interval};
use tokio_util::sync::CancellationToken;

#[cfg(feature = "with_tracing")]
use tracing::{debug, error, info, warn};

use crate::{
    supervisor::handle::{SupervisorHandle, SupervisorMessage},
    task::{TaskHandle, TaskResult, TaskStatus},
};

#[derive(Clone, Debug, thiserror::Error)]
pub enum SupervisorError {
    #[error("Too many tasks are dead (threshold exceeded: {current_percentage:.2}% > {threshold:.2}%), supervisor shutting down.")]
    TooManyDeadTasks {
        current_percentage: f64,
        threshold: f64,
    },
}

/// Internal messages sent from tasks to the supervisor.
#[derive(Debug)]
pub(crate) enum SupervisedTaskMessage {
    /// Sent when a task completes, either successfully or with a failure.
    Completed(Arc<str>, TaskResult),
    /// Sent when a shutdown signal is requested by the user.
    Shutdown,
}

/// A pending restart, ordered by deadline (earliest first).
struct PendingRestart {
    deadline: tokio::time::Instant,
    task_name: Arc<str>,
}

impl PartialEq for PendingRestart {
    fn eq(&self, other: &Self) -> bool {
        self.deadline == other.deadline
    }
}

impl Eq for PendingRestart {}

impl PartialOrd for PendingRestart {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PendingRestart {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse ordering so BinaryHeap (max-heap) pops the earliest deadline first.
        other.deadline.cmp(&self.deadline)
    }
}

/// Manages a set of tasks, ensuring they remain operational through restarts.
///
/// The `Supervisor` spawns each task with monitoring to detect failures.
/// If a task fails, it is restarted with an exponential backoff.
/// User commands such as adding, restarting, or killing tasks are supported via the `SupervisorHandle`.
pub struct Supervisor {
    pub(crate) tasks: HashMap<Arc<str>, TaskHandle>,
    // Durations for tasks lifecycle
    pub(crate) health_check_interval: Duration,
    pub(crate) base_restart_delay: Duration,
    pub(crate) task_is_stable_after: Duration,
    pub(crate) max_restart_attempts: Option<u32>,
    pub(crate) max_backoff_exponent: u32,
    pub(crate) max_dead_tasks_percentage_threshold: Option<f64>,
    // Channels between the User & the Supervisor
    pub(crate) external_tx: mpsc::UnboundedSender<SupervisorMessage>,
    pub(crate) external_rx: mpsc::UnboundedReceiver<SupervisorMessage>,
    // Internal channel: tasks -> supervisor
    pub(crate) internal_tx: mpsc::UnboundedSender<SupervisedTaskMessage>,
    pub(crate) internal_rx: mpsc::UnboundedReceiver<SupervisedTaskMessage>,
}

impl Supervisor {
    /// Runs the supervisor, consuming it and returning a handle for external control.
    pub fn run(self) -> SupervisorHandle {
        let user_tx = self.external_tx.clone();
        let handle = tokio::spawn(async move { self.run_and_supervise().await });
        SupervisorHandle::new(handle, user_tx)
    }

    async fn run_and_supervise(mut self) -> Result<(), SupervisorError> {
        self.start_all_tasks();
        self.supervise_all_tasks().await
    }

    fn start_all_tasks(&mut self) {
        let task_names: Vec<Arc<str>> = self.tasks.keys().cloned().collect();
        for task_name in task_names {
            self.start_task(&task_name);
        }
    }

    /// Main supervision loop.
    async fn supervise_all_tasks(&mut self) -> Result<(), SupervisorError> {
        let mut health_check_ticker = interval(self.health_check_interval);
        let mut pending_restarts: BinaryHeap<PendingRestart> = BinaryHeap::new();

        loop {
            // Compute the sleep for the next pending restart (if any).
            let next_restart = async {
                match pending_restarts.peek() {
                    Some(pr) => tokio::time::sleep_until(pr.deadline).await,
                    None => std::future::pending().await,
                }
            };

            tokio::select! {
                biased;
                Some(internal_msg) = self.internal_rx.recv() => {
                    match internal_msg {
                        SupervisedTaskMessage::Shutdown => {
                            #[cfg(feature = "with_tracing")]
                            info!("Supervisor received shutdown signal");
                            return Ok(());
                        }
                        SupervisedTaskMessage::Completed(task_name, outcome) => {
                            #[cfg(feature = "with_tracing")]
                            match &outcome {
                                Ok(()) => info!("Task '{}' completed successfully", task_name),
                                Err(e) => warn!("Task '{}' completed with error: {e}", task_name),
                            }
                            self.handle_task_completion(&task_name, outcome, &mut pending_restarts);
                        }
                    }
                },
                Some(user_msg) = self.external_rx.recv() => {
                    self.handle_user_message(user_msg, &mut pending_restarts);
                },
                _ = next_restart => {
                    // Pop and execute the restart whose deadline has arrived.
                    if let Some(pr) = pending_restarts.pop() {
                        self.restart_task(&pr.task_name);
                    }
                },
                _ = health_check_ticker.tick() => {
                    #[cfg(feature = "with_tracing")]
                    debug!("Supervisor checking health of all tasks");
                    self.check_all_health(&mut pending_restarts);
                    self.check_dead_tasks_threshold()?;
                }
            }
        }
    }

    /// Processes user commands received via the `SupervisorHandle`.
    fn handle_user_message(
        &mut self,
        msg: SupervisorMessage,
        pending_restarts: &mut BinaryHeap<PendingRestart>,
    ) {
        match msg {
            SupervisorMessage::AddTask(task_name, task_dyn) => {
                let key: Arc<str> = Arc::from(task_name);

                // TODO: This branch should return an error
                if self.tasks.contains_key(&key) {
                    #[cfg(feature = "with_tracing")]
                    warn!("Attempted to add task '{}' but it already exists", key);
                    return;
                }

                let mut task_handle = TaskHandle::new(task_dyn);
                task_handle.max_restart_attempts = self.max_restart_attempts;
                task_handle.base_restart_delay = self.base_restart_delay;
                task_handle.max_backoff_exponent = self.max_backoff_exponent;

                self.tasks.insert(Arc::clone(&key), task_handle);
                self.start_task(&key);
            }
            SupervisorMessage::RestartTask(task_name) => {
                let key: Arc<str> = Arc::from(task_name);
                #[cfg(feature = "with_tracing")]
                info!("User requested restart for task: {}", key);
                self.restart_task(&key);
            }
            SupervisorMessage::KillTask(task_name) => {
                let key: Arc<str> = Arc::from(task_name);
                if let Some(task_handle) = self.tasks.get_mut(&key) {
                    if task_handle.status != TaskStatus::Dead {
                        task_handle.mark(TaskStatus::Dead);
                        task_handle.clean();
                    }
                } else {
                    #[cfg(feature = "with_tracing")]
                    warn!("Attempted to kill non-existent task: {}", key);
                }
            }
            SupervisorMessage::GetTaskStatus(task_name, sender) => {
                let key: Arc<str> = Arc::from(task_name);
                let status = self.tasks.get(&key).map(|handle| handle.status);

                #[cfg(feature = "with_tracing")]
                debug!("Status query for task '{}': {:?}", key, status);

                let _ = sender.send(status);
            }
            SupervisorMessage::GetAllTaskStatuses(sender) => {
                let statuses = self
                    .tasks
                    .iter()
                    .map(|(name, handle)| (String::from(name.as_ref()), handle.status))
                    .collect();
                let _ = sender.send(statuses);
            }
            SupervisorMessage::Shutdown => {
                #[cfg(feature = "with_tracing")]
                info!("User requested supervisor shutdown");

                for (_, task_handle) in self.tasks.iter_mut() {
                    if task_handle.status != TaskStatus::Dead
                        && task_handle.status != TaskStatus::Completed
                    {
                        task_handle.clean();
                        task_handle.mark(TaskStatus::Dead);
                    }
                }
                pending_restarts.clear();
                let _ = self.internal_tx.send(SupervisedTaskMessage::Shutdown);
            }
        }
    }

    /// Starts a task, spawning a single tokio task that runs it and reports completion.
    fn start_task(&mut self, task_name: &Arc<str>) {
        let Some(task_handle) = self.tasks.get_mut(task_name) else {
            return;
        };

        task_handle.mark(TaskStatus::Healthy);

        let token = CancellationToken::new();
        task_handle.cancellation_token = Some(token.clone());

        let mut task_instance = task_handle.task.clone_box();
        let internal_tx = self.internal_tx.clone();
        let name = Arc::clone(task_name);

        let join_handle = tokio::spawn(async move {
            tokio::select! {
                _ = token.cancelled() => { }
                result = task_instance.run_boxed() => {
                    let _ = internal_tx.send(SupervisedTaskMessage::Completed(name, result));
                }
            }
        });

        task_handle.join_handle = Some(join_handle);
    }

    /// Restarts a task after cleaning up its previous execution.
    fn restart_task(&mut self, task_name: &Arc<str>) {
        if let Some(task_handle) = self.tasks.get_mut(task_name) {
            task_handle.clean();
        }
        self.start_task(task_name);
    }

    fn check_all_health(&mut self, pending_restarts: &mut BinaryHeap<PendingRestart>) {
        let now = Instant::now();

        // First pass: mark failed tasks and collect their names.
        // We collect into a fixed-capacity buffer to avoid per-tick heap allocation
        // in the common case where few (or zero) tasks need restart.
        let mut failed_names: Vec<Arc<str>> = Vec::new();

        for (task_name, task_handle) in self.tasks.iter_mut() {
            if task_handle.status != TaskStatus::Healthy {
                continue;
            }

            if let Some(handle) = &task_handle.join_handle {
                if handle.is_finished() {
                    #[cfg(feature = "with_tracing")]
                    warn!(
                        "Task '{}' unexpectedly finished, marking as failed",
                        task_name
                    );

                    task_handle.mark(TaskStatus::Failed);
                    failed_names.push(Arc::clone(task_name));
                } else {
                    // Task is running. Check for stability — reset restart counter
                    // once a task has been healthy long enough.
                    if let Some(healthy_since) = task_handle.healthy_since {
                        if now.duration_since(healthy_since) > self.task_is_stable_after
                            && task_handle.restart_attempts > 0
                        {
                            #[cfg(feature = "with_tracing")]
                            info!(
                                "Task '{}' is now stable, resetting restart attempts",
                                task_name
                            );
                            task_handle.restart_attempts = 0;
                        }
                    } else {
                        task_handle.healthy_since = Some(now);
                    }
                }
            } else {
                #[cfg(feature = "with_tracing")]
                error!("Task '{}' has no join handle, marking as failed", task_name);

                task_handle.mark(TaskStatus::Failed);
                failed_names.push(Arc::clone(task_name));
            }
        }

        for task_name in failed_names {
            self.schedule_restart_or_kill(&task_name, pending_restarts);
        }
    }

    fn handle_task_completion(
        &mut self,
        task_name: &Arc<str>,
        outcome: TaskResult,
        pending_restarts: &mut BinaryHeap<PendingRestart>,
    ) {
        let Some(task_handle) = self.tasks.get_mut(task_name) else {
            #[cfg(feature = "with_tracing")]
            warn!("Received completion for non-existent task: {}", task_name);
            return;
        };

        task_handle.clean();

        match outcome {
            Ok(()) => {
                #[cfg(feature = "with_tracing")]
                info!("Task '{}' completed successfully", task_name);

                task_handle.mark(TaskStatus::Completed);
            }
            #[allow(unused_variables)]
            Err(ref e) => {
                #[cfg(feature = "with_tracing")]
                error!("Task '{}' failed with error: {:?}", task_name, e);

                task_handle.mark(TaskStatus::Failed);
                self.schedule_restart_or_kill(task_name, pending_restarts);
            }
        }
    }

    /// Shared logic for scheduling a restart with backoff, or marking the task dead
    /// if max retries have been exceeded.
    fn schedule_restart_or_kill(
        &mut self,
        task_name: &Arc<str>,
        pending_restarts: &mut BinaryHeap<PendingRestart>,
    ) {
        let Some(task_handle) = self.tasks.get_mut(task_name) else {
            return;
        };

        if task_handle.has_exceeded_max_retries() {
            #[cfg(feature = "with_tracing")]
            error!(
                "Task '{}' exceeded max restart attempts ({:?}), marking as dead",
                task_name,
                task_handle
                    .max_restart_attempts
                    .expect("is provided if has exceeded")
            );

            task_handle.mark(TaskStatus::Dead);
            task_handle.clean();
            return;
        }

        task_handle.restart_attempts = task_handle.restart_attempts.saturating_add(1);
        let restart_delay = task_handle.restart_delay();

        #[cfg(feature = "with_tracing")]
        info!(
            "Scheduling restart for task '{}' in {:?} (attempt {}/{})",
            task_name,
            restart_delay,
            task_handle.restart_attempts,
            task_handle
                .max_restart_attempts
                .map(|t| t.to_string())
                .unwrap_or_else(|| "\u{221e}".to_string())
        );

        pending_restarts.push(PendingRestart {
            deadline: tokio::time::Instant::now() + restart_delay,
            task_name: Arc::clone(task_name),
        });
    }

    fn check_dead_tasks_threshold(&mut self) -> Result<(), SupervisorError> {
        let Some(threshold) = self.max_dead_tasks_percentage_threshold else {
            return Ok(());
        };

        let total_task_count = self.tasks.len();
        if total_task_count == 0 {
            return Ok(());
        }

        // Single-pass: count dead tasks.
        let dead_task_count = self
            .tasks
            .values()
            .filter(|handle| handle.status == TaskStatus::Dead)
            .count();

        let current_dead_percentage = dead_task_count as f64 / total_task_count as f64;

        if current_dead_percentage <= threshold {
            return Ok(());
        }

        #[cfg(feature = "with_tracing")]
        error!(
            "Dead tasks threshold exceeded: {:.2}% > {:.2}% ({}/{} tasks dead)",
            current_dead_percentage * 100.0,
            threshold * 100.0,
            dead_task_count,
            total_task_count
        );

        // Kill all remaining non-dead/non-completed tasks
        #[allow(unused_variables)]
        for (task_name, task_handle) in self.tasks.iter_mut() {
            if task_handle.status != TaskStatus::Dead && task_handle.status != TaskStatus::Completed
            {
                #[cfg(feature = "with_tracing")]
                debug!("Killing task '{}' due to threshold breach", task_name);

                task_handle.clean();
                task_handle.mark(TaskStatus::Dead);
            }
        }

        Err(SupervisorError::TooManyDeadTasks {
            current_percentage: current_dead_percentage,
            threshold,
        })
    }
}
