pub(crate) mod builder;
pub(crate) mod handle;

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use handle::SupervisorMessage;
use tokio::{sync::mpsc, time::interval_at};
use tokio_util::sync::CancellationToken;

use crate::{
    supervisor::handle::SupervisorHandle,
    task::{TaskHandle, TaskOutcome, TaskStatus},
};

/// A heartbeat sent from a task to indicate that it's alive.
#[derive(Debug, Clone)]
pub(crate) struct Heartbeat {
    pub(crate) task_name: String,
    pub(crate) timestamp: Instant,
}

impl Heartbeat {
    /// Creates a new heartbeat for the given task name with the current timestamp.
    pub fn new(task_name: &String) -> Self {
        Self {
            task_name: task_name.to_string(),
            timestamp: Instant::now(),
        }
    }
}

/// Internal messages sent from tasks and by the `Supervisor` to manage task lifecycle.
#[derive(Debug)]
pub(crate) enum SupervisedTaskMessage {
    /// Sent by tasks to indicate they are alive.
    Heartbeat(Heartbeat),
    /// Sent by the supervisor to trigger a task restart.
    Restart(String),
    /// Sent when a task completes, either successfully or with a failure.
    Completed(String, TaskOutcome),
    /// Sent when a shutdown signal is asked by the User. Close every tasks & the supervisor.
    Shutdown,
}

/// Manages a set of tasks, ensuring they remain operational through heartbeats and restarts.
///
/// The `Supervisor` spawns each task with a heartbeat mechanism to monitor liveness.
/// If a task stops sending heartbeats or fails, it is restarted with an exponential backoff.
/// User commands such as adding, restarting, or killing tasks are supported via the `SupervisorHandle`.
pub struct Supervisor {
    // List of current tasks
    tasks: HashMap<String, TaskHandle>,
    // Durations for tasks lifecycle
    timeout_threshold: Duration,
    heartbeat_interval: Duration,
    health_check_initial_delay: Duration,
    health_check_interval: Duration,
    base_restart_delay: Duration,
    task_is_stable_after: Duration,
    max_restart_attempts: u32,
    // Channels between the User & the Supervisor
    external_tx: mpsc::UnboundedSender<SupervisorMessage>,
    external_rx: mpsc::UnboundedReceiver<SupervisorMessage>,
    // Channels used in the Supervisor internally (between threads)
    internal_tx: mpsc::UnboundedSender<SupervisedTaskMessage>,
    internal_rx: mpsc::UnboundedReceiver<SupervisedTaskMessage>,
}

impl Supervisor {
    /// Runs the supervisor, consuming it and returning a handle for external control.
    ///
    /// This method initiates all tasks and starts the supervision loop.
    pub fn run(self) -> SupervisorHandle {
        let user_tx = self.external_tx.clone();
        let handle = tokio::spawn(async move {
            self.run_and_supervise().await;
        });
        SupervisorHandle::new(handle, user_tx)
    }

    /// Starts and supervises all tasks, running the main supervision loop.
    async fn run_and_supervise(mut self) {
        self.start_all_tasks().await;
        self.supervise_all_tasks().await;
    }

    /// Initiates all tasks managed by the supervisor.
    async fn start_all_tasks(&mut self) {
        for (task_name, task_handle) in self.tasks.iter_mut() {
            Self::start_task(
                task_name.to_string(),
                task_handle,
                self.internal_tx.clone(),
                self.heartbeat_interval,
            )
            .await;
        }
    }

    /// Supervises tasks by processing messages and performing periodic health checks.
    async fn supervise_all_tasks(&mut self) {
        let mut health_check_interval = interval_at(
            tokio::time::Instant::now() + self.health_check_initial_delay,
            self.health_check_interval,
        );

        loop {
            tokio::select! {
                Some(internal_msg) = self.internal_rx.recv() => {
                    // Exit the supervising loop
                    if matches!(internal_msg, SupervisedTaskMessage::Shutdown) {
                        return;
                    }
                    self.handle_internal_message(internal_msg).await;
                },
                Some(user_msg) = self.external_rx.recv() => {
                    self.handle_user_message(user_msg).await;
                },
                _ = health_check_interval.tick() => {
                    self.check_all_health();
                }
            }
        }
    }

    async fn handle_internal_message(&mut self, msg: SupervisedTaskMessage) {
        match msg {
            SupervisedTaskMessage::Heartbeat(heartbeat) => {
                self.register_heartbeat(heartbeat);
            }
            SupervisedTaskMessage::Restart(task_name) => {
                self.restart_task(task_name).await;
            }
            SupervisedTaskMessage::Completed(task_name, outcome) => {
                self.handle_task_completion(task_name, outcome).await;
            }
            SupervisedTaskMessage::Shutdown => unreachable!(),
        }
    }

    /// Processes user commands received via the `SupervisorHandle`.
    async fn handle_user_message(&mut self, msg: SupervisorMessage) {
        match msg {
            SupervisorMessage::AddTask(task_name, task) => {
                if self.tasks.contains_key(&task_name) {
                    return;
                }
                let mut task_handle =
                    TaskHandle::new(task, self.max_restart_attempts, self.base_restart_delay);
                Self::start_task(
                    task_name.clone(),
                    &mut task_handle,
                    self.internal_tx.clone(),
                    self.heartbeat_interval,
                )
                .await;
                self.tasks.insert(task_name, task_handle);
            }
            SupervisorMessage::RestartTask(task_name) => {
                self.restart_task(task_name).await;
            }
            SupervisorMessage::KillTask(task_name) => {
                let Some(task_handle) = self.tasks.get_mut(&task_name) else {
                    return;
                };
                if task_handle.status == TaskStatus::Dead {
                    return;
                }
                task_handle.mark(TaskStatus::Dead);
                task_handle.clean().await;
            }
            SupervisorMessage::GetTaskStatus(task_name, sender) => {
                let status = self.tasks.get(&task_name).map(|handle| handle.status);
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
                for (_, task_handle) in self.tasks.iter_mut() {
                    if task_handle.status != TaskStatus::Dead {
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
        heartbeat_interval: Duration,
    ) {
        let token = CancellationToken::new();

        // Completion task
        let (completion_tx, mut completion_rx) = mpsc::channel::<TaskOutcome>(1);
        let tx_heartbeat = internal_tx.clone();
        let task_name_clone = task_name.clone();
        let token_completion = token.clone();
        let completion_task = tokio::spawn(async move {
            tokio::select! {
                _ = token_completion.cancelled() => {
                    // Task cancelled
                }
                Some(outcome) = completion_rx.recv() => {
                    let completion_msg = SupervisedTaskMessage::Completed(task_name_clone, outcome);
                    let _ = tx_heartbeat.send(completion_msg);
                    token_completion.cancel(); // Cancel other tasks upon completion
                }
            }
        });

        // Heartbeat task
        let task_name_heartbeat = task_name.clone();
        let token_heartbeat = token.clone();
        let heartbeat_task = tokio::spawn(async move {
            let mut beat_interval = tokio::time::interval(heartbeat_interval);
            loop {
                tokio::select! {
                    _ = beat_interval.tick() => {
                        let beat = SupervisedTaskMessage::Heartbeat(Heartbeat::new(&task_name_heartbeat));
                        if internal_tx.send(beat).is_err() {
                            break;
                        }
                    }
                    _ = token_heartbeat.cancelled() => {
                        break; // Stop heartbeat on cancellation
                    }
                }
            }
        });

        // Main task
        let mut task = task_handle.task.clone_box();
        let token_main = token.clone();
        let ran_task = tokio::spawn(async move {
            tokio::select! {
                _ = token_main.cancelled() => {
                    // Task cancelled
                }
                _ = async {
                    match task.run().await {
                        Ok(outcome) => {
                            let _ = completion_tx.send(outcome).await;
                        }
                        Err(e) => {
                            let _ = completion_tx.send(TaskOutcome::Failed(e.to_string())).await;
                        }
                    }
                } => {}
            }
        });

        // Mark the task as `Starting`
        task_handle.mark(TaskStatus::Starting);
        // Store the token and handles in TaskHandle
        task_handle.cancellation_token = Some(token);
        task_handle.handles = Some(vec![ran_task, heartbeat_task, completion_task]);
    }

    /// Updates a task's status based on received heartbeats.
    fn register_heartbeat(&mut self, heartbeat: Heartbeat) {
        let Some(task_handle) = self.tasks.get_mut(&heartbeat.task_name) else {
            return;
        };

        if task_handle.status == TaskStatus::Dead {
            return;
        }

        task_handle.ticked_at(heartbeat.timestamp);

        match task_handle.status {
            TaskStatus::Starting => {
                task_handle.mark(TaskStatus::Healthy);
                task_handle.healthy_since = Some(heartbeat.timestamp);
            }
            TaskStatus::Healthy => {
                // Reset the `restart_attempts` if the task has been healthy & stable for some time
                if let Some(healthy_since) = task_handle.healthy_since {
                    if heartbeat.timestamp.duration_since(healthy_since) > self.task_is_stable_after
                    {
                        task_handle.restart_attempts = 0;
                    }
                } else {
                    task_handle.healthy_since = Some(heartbeat.timestamp);
                }
            }
            _ => {}
        }
    }

    /// Restarts a task after cleaning up its previous execution.
    async fn restart_task(&mut self, task_name: String) {
        let Some(task_handle) = self.tasks.get_mut(&task_name) else {
            return;
        };
        task_handle.clean().await;
        task_handle.mark(TaskStatus::Created);
        Self::start_task(
            task_name,
            task_handle,
            self.internal_tx.clone(),
            self.heartbeat_interval,
        )
        .await;
    }

    /// Checks task health and schedules restarts for crashed tasks.
    fn check_all_health(&mut self) {
        let crashed_tasks = self
            .tasks
            .iter()
            .filter(|(_, handle)| handle.has_crashed(self.timeout_threshold))
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();

        for crashed_task in crashed_tasks {
            let Some(task_handle) = self.tasks.get_mut(&crashed_task) else {
                continue;
            };
            if task_handle.has_exceeded_max_retries() && task_handle.status != TaskStatus::Dead {
                task_handle.mark(TaskStatus::Dead);
                continue;
            }
            let restart_delay = task_handle.restart_delay();
            task_handle.mark(TaskStatus::Failed);
            task_handle.restart_attempts = task_handle.restart_attempts.saturating_add(1);
            let internal_tx = self.internal_tx.clone();
            tokio::spawn(async move {
                tokio::time::sleep(restart_delay).await;
                let _ = internal_tx.send(SupervisedTaskMessage::Restart(crashed_task));
            });
        }
    }

    /// Handles task completion outcomes, deciding whether to mark as completed or restart.
    async fn handle_task_completion(&mut self, task_name: String, outcome: TaskOutcome) {
        let Some(task_handle) = self.tasks.get_mut(&task_name) else {
            return;
        };

        match outcome {
            TaskOutcome::Completed => {
                task_handle.mark(TaskStatus::Completed);
            }
            TaskOutcome::Failed(_) => {
                task_handle.mark(TaskStatus::Failed);
                if task_handle.has_exceeded_max_retries() {
                    task_handle.mark(TaskStatus::Dead);
                    return;
                }
                let restart_delay = task_handle.restart_delay();
                task_handle.restart_attempts = task_handle.restart_attempts.saturating_add(1);
                let internal_tx = self.internal_tx.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(restart_delay).await;
                    let _ = internal_tx.send(SupervisedTaskMessage::Restart(task_name));
                });
            }
        }
    }
}
