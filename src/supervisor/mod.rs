pub(crate) mod builder;
pub(crate) mod handle;

use std::{
    collections::HashMap,
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

/// Internal messages sent from tasks and by the `Supervisor` to manage task lifecycle.
#[derive(Debug)]
pub(crate) enum SupervisedTaskMessage {
    /// Sent by the supervisor to itself to trigger a task restart.
    Restart(String),
    /// Sent when a task completes, either successfully or with a failure.
    Completed(String, TaskResult),
    /// Sent when a shutdown signal is asked by the User. Close every tasks & the supervisor.
    Shutdown,
}

/// Manages a set of tasks, ensuring they remain operational through restarts.
///
/// The `Supervisor` spawns each task with a heartbeat mechanism to monitor liveness.
/// If a task stops sending heartbeats or fails, it is restarted with an exponential backoff.
/// User commands such as adding, restarting, or killing tasks are supported via the `SupervisorHandle`.
pub struct Supervisor {
    // List of current tasks
    tasks: HashMap<String, TaskHandle>,
    // Durations for tasks lifecycle
    health_check_interval: Duration,
    base_restart_delay: Duration,
    task_is_stable_after: Duration,
    max_restart_attempts: u32,
    max_dead_tasks_percentage_threshold: Option<f64>,
    // Channels between the User & the Supervisor
    external_tx: mpsc::UnboundedSender<SupervisorMessage>,
    external_rx: mpsc::UnboundedReceiver<SupervisorMessage>,
    // Internal channels used in the Supervisor for actions propagation
    internal_tx: mpsc::UnboundedSender<SupervisedTaskMessage>,
    internal_rx: mpsc::UnboundedReceiver<SupervisedTaskMessage>,
}

impl Supervisor {
    /// Runs the supervisor, consuming it and returning a handle for external control.
    ///
    /// This method initiates all tasks and starts the supervision loop.
    pub fn run(self) -> SupervisorHandle {
        let user_tx = self.external_tx.clone();
        let handle = tokio::spawn(async move { self.run_and_supervise().await });
        SupervisorHandle::new(handle, user_tx)
    }

    /// Starts and supervises all tasks, running the main supervision loop.
    async fn run_and_supervise(mut self) -> Result<(), SupervisorError> {
        self.start_all_tasks().await;
        self.supervise_all_tasks().await
    }

    /// Initiates all tasks managed by the supervisor.
    async fn start_all_tasks(&mut self) {
        for (task_name, task_handle) in self.tasks.iter_mut() {
            Self::start_task(task_name.to_string(), task_handle, self.internal_tx.clone()).await;
        }
    }

    /// Supervises tasks by processing messages and performing periodic health checks.
    async fn supervise_all_tasks(&mut self) -> Result<(), SupervisorError> {
        let mut health_check_ticker = interval(self.health_check_interval);

        loop {
            tokio::select! {
                biased;
                Some(internal_msg) = self.internal_rx.recv() => {
                    match internal_msg {
                        SupervisedTaskMessage::Shutdown => {
                            #[cfg(feature = "with_tracing")]
                            info!("Supervisor received shutdown signal");
                            return Ok(());
                        }
                        _ => self.handle_internal_message(internal_msg).await,
                    }
                },
                Some(user_msg) = self.external_rx.recv() => {
                    self.handle_user_message(user_msg).await;
                },
                _ = health_check_ticker.tick() => {
                    #[cfg(feature = "with_tracing")]
                    debug!("Supervisor checking health of all tasks");
                    self.check_all_health().await;
                    self.check_dead_tasks_threshold().await?;
                }
            }
        }
    }

    async fn handle_internal_message(&mut self, msg: SupervisedTaskMessage) {
        match msg {
            SupervisedTaskMessage::Restart(task_name) => {
                #[cfg(feature = "with_tracing")]
                info!("Processing restart request for task: {task_name}");
                self.restart_task(task_name).await;
            }
            SupervisedTaskMessage::Completed(task_name, outcome) => {
                #[cfg(feature = "with_tracing")]
                match &outcome {
                    Ok(()) => info!("Task '{task_name}' completed successfully"),
                    Err(e) => warn!("Task '{task_name}' completed with error: {e}"),
                }
                self.handle_task_completion(task_name, outcome).await;
            }
            SupervisedTaskMessage::Shutdown => {
                unreachable!("Shutdown is handled by the main select loop.");
            }
        }
    }

    /// Processes user commands received via the `SupervisorHandle`.
    async fn handle_user_message(&mut self, msg: SupervisorMessage) {
        match msg {
            SupervisorMessage::AddTask(task_name, task_dyn) => {
                // TODO: This branch should return an error
                if self.tasks.contains_key(&task_name) {
                    #[cfg(feature = "with_tracing")]
                    warn!("Attempted to add task '{task_name}' but it already exists");
                    return;
                }

                let mut task_handle =
                    TaskHandle::new(task_dyn, self.max_restart_attempts, self.base_restart_delay);
                Self::start_task(
                    task_name.clone(),
                    &mut task_handle,
                    self.internal_tx.clone(),
                )
                .await;
                self.tasks.insert(task_name, task_handle);
            }
            SupervisorMessage::RestartTask(task_name) => {
                #[cfg(feature = "with_tracing")]
                info!("User requested restart for task: {task_name}");
                self.restart_task(task_name).await;
            }
            SupervisorMessage::KillTask(task_name) => {
                if let Some(task_handle) = self.tasks.get_mut(&task_name) {
                    if task_handle.status != TaskStatus::Dead {
                        task_handle.mark(TaskStatus::Dead);
                        task_handle.clean().await;
                    }
                } else {
                    #[cfg(feature = "with_tracing")]
                    warn!("Attempted to kill non-existent task: {task_name}");
                }
            }
            SupervisorMessage::GetTaskStatus(task_name, sender) => {
                let status = self.tasks.get(&task_name).map(|handle| handle.status);

                #[cfg(feature = "with_tracing")]
                debug!("Status query for task '{task_name}': {status:?}");

                let _ = sender.send(status);
            }
            SupervisorMessage::GetAllTaskStatuses(sender) => {
                let statuses = self
                    .tasks
                    .iter()
                    .map(|(name, handle)| (name.clone(), handle.status))
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
                        task_handle.clean().await;
                        task_handle.mark(TaskStatus::Dead);
                    }
                }
                let _ = self.internal_tx.send(SupervisedTaskMessage::Shutdown);
            }
        }
    }

    /// Starts a task, setting up its execution, heartbeat, and completion handling.
    async fn start_task(
        task_name: String,
        task_handle: &mut TaskHandle,
        internal_tx: mpsc::UnboundedSender<SupervisedTaskMessage>,
    ) {
        task_handle.started_at = Some(Instant::now());
        task_handle.mark(TaskStatus::Healthy);

        let token = CancellationToken::new();
        task_handle.cancellation_token = Some(token.clone());

        let (completion_tx, mut completion_rx) = mpsc::channel::<TaskResult>(1);

        // Completion Listener Task
        let task_name_completion = task_name.clone();
        let token_completion = token.clone();
        let internal_tx_completion = internal_tx.clone();
        let completion_listener_handle = tokio::spawn(async move {
            tokio::select! {
                _ = token_completion.cancelled() => { }
                Some(outcome) = completion_rx.recv() => {
                    let completion_msg = SupervisedTaskMessage::Completed(task_name_completion.clone(), outcome);
                    let _ = internal_tx_completion.send(completion_msg);
                    token_completion.cancel();
                }
            }
        });

        // Main Task Execution
        let mut task_instance = task_handle.task.clone_box();
        let token_main = token.clone();
        let main_task_execution_handle = tokio::spawn(async move {
            tokio::select! {
                _ = token_main.cancelled() => { }
                run_result = task_instance.run() => {
                    let _ = completion_tx.send(run_result).await;
                }
            }
        });

        task_handle.main_task_handle = Some(main_task_execution_handle);
        task_handle.completion_task_handle = Some(completion_listener_handle);
    }

    /// Restarts a task after cleaning up its previous execution.
    async fn restart_task(&mut self, task_name: String) {
        if let Some(task_handle) = self.tasks.get_mut(&task_name) {
            task_handle.clean().await;
            Self::start_task(task_name, task_handle, self.internal_tx.clone()).await;
        }
    }

    async fn check_all_health(&mut self) {
        let mut tasks_needing_restart: Vec<String> = Vec::new();
        let now = Instant::now();

        for (task_name, task_handle) in self.tasks.iter_mut() {
            if task_handle.status == TaskStatus::Healthy {
                if let Some(main_handle) = &task_handle.main_task_handle {
                    if main_handle.is_finished() {
                        #[cfg(feature = "with_tracing")]
                        warn!("Task '{task_name}' unexpectedly finished, marking as failed");

                        task_handle.mark(TaskStatus::Failed);
                        tasks_needing_restart.push(task_name.clone());
                    } else {
                        // Task is Healthy and running. Check for stability.
                        if let Some(healthy_since) = task_handle.healthy_since {
                            if (now.duration_since(healthy_since) > self.task_is_stable_after)
                                && task_handle.restart_attempts > 0
                            {
                                #[cfg(feature = "with_tracing")]
                                info!(
                                    "Task '{task_name}' is now stable, resetting restart attempts",
                                );

                                task_handle.restart_attempts = 0;
                            }
                        } else {
                            task_handle.healthy_since = Some(now);
                        }
                    }
                } else {
                    #[cfg(feature = "with_tracing")]
                    error!("Task '{task_name}' has no main handle, marking as failed");

                    task_handle.mark(TaskStatus::Failed);
                    tasks_needing_restart.push(task_name.clone());
                }
            }
        }

        for task_name in tasks_needing_restart {
            let Some(task_handle) = self.tasks.get_mut(&task_name) else {
                continue;
            };

            // Ensure it's still failed
            if task_handle.has_exceeded_max_retries() {
                #[cfg(feature = "with_tracing")]
                error!(
                    "Task '{task_name}' exceeded max restart attempts ({}), marking as dead",
                    self.max_restart_attempts
                );

                task_handle.mark(TaskStatus::Dead);
                task_handle.clean().await;
                continue;
            }

            // Increment before calculating delay for the current attempt
            task_handle.restart_attempts = task_handle.restart_attempts.saturating_add(1);
            let restart_delay = task_handle.restart_delay();

            #[cfg(feature = "with_tracing")]
            info!(
                "Scheduling restart for task '{task_name}' in {restart_delay:?} (attempt {}/{})",
                task_handle.restart_attempts, self.max_restart_attempts
            );

            let internal_tx_clone = self.internal_tx.clone();
            tokio::spawn(async move {
                tokio::time::sleep(restart_delay).await;
                let _ = internal_tx_clone.send(SupervisedTaskMessage::Restart(task_name.clone()));
            });
        }
    }

    async fn handle_task_completion(&mut self, task_name: String, outcome: TaskResult) {
        let Some(task_handle) = self.tasks.get_mut(&task_name) else {
            #[cfg(feature = "with_tracing")]
            warn!("Received completion for non-existent task: {}", task_name);
            return;
        };

        task_handle.clean().await;

        match outcome {
            Ok(()) => {
                #[cfg(feature = "with_tracing")]
                info!("Task '{task_name}' completed successfully");

                task_handle.mark(TaskStatus::Completed);
            }
            #[allow(unused_variables)]
            Err(ref e) => {
                #[cfg(feature = "with_tracing")]
                error!("Task '{task_name}' failed with error: {e:?}");

                task_handle.mark(TaskStatus::Failed);
                if task_handle.has_exceeded_max_retries() {
                    #[cfg(feature = "with_tracing")]
                    error!(
                        "Task '{task_name}' exceeded max restart attempts after failure, marking as dead",
                    );

                    task_handle.mark(TaskStatus::Dead);
                    return;
                }

                task_handle.restart_attempts = task_handle.restart_attempts.saturating_add(1);
                let restart_delay = task_handle.restart_delay();

                #[cfg(feature = "with_tracing")]
                info!(
                    "Scheduling restart for failed task '{task_name}' in {restart_delay:?} (attempt {}/{})",
                    task_handle.restart_attempts,
                    self.max_restart_attempts
                );

                let internal_tx_clone = self.internal_tx.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(restart_delay).await;
                    let _ =
                        internal_tx_clone.send(SupervisedTaskMessage::Restart(task_name.clone()));
                });
            }
        }
    }

    async fn check_dead_tasks_threshold(&mut self) -> Result<(), SupervisorError> {
        if let Some(threshold) = self.max_dead_tasks_percentage_threshold {
            if !self.tasks.is_empty() {
                let dead_task_count = self
                    .tasks
                    .values()
                    .filter(|handle| handle.status == TaskStatus::Dead)
                    .count();

                let total_task_count = self.tasks.len();
                let current_dead_percentage = dead_task_count as f64 / total_task_count as f64;

                if current_dead_percentage > threshold {
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
                        if task_handle.status != TaskStatus::Dead
                            && task_handle.status != TaskStatus::Completed
                        {
                            #[cfg(feature = "with_tracing")]
                            debug!("Killing task '{task_name}' due to threshold breach");

                            task_handle.clean().await;
                            task_handle.mark(TaskStatus::Dead);
                        }
                    }

                    return Err(SupervisorError::TooManyDeadTasks {
                        current_percentage: current_dead_percentage,
                        threshold,
                    });
                }
            }
        };

        Ok(())
    }
}
