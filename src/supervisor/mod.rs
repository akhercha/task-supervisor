pub(crate) mod builder;
pub(crate) mod handle;

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use handle::SupervisorMessage;
use tokio::{sync::mpsc, time::interval_at};

use crate::{
    supervisor::handle::SupervisorHandle,
    task::{TaskHandle, TaskOutcome, TaskStatus},
    utils::TaskGroup,
    TaskName,
};

/// A beat sent from a task to indicate that it's alive.
#[derive(Debug, Clone)]
pub(crate) struct Heartbeat {
    pub(crate) task_name: TaskName,
    pub(crate) timestamp: Instant,
}

impl Heartbeat {
    pub fn new(task_name: &TaskName) -> Self {
        Self {
            task_name: task_name.to_string(),
            timestamp: Instant::now(),
        }
    }
}

/// Internal messages sent from tasks & by the `Supervisor` itself to restart
/// tasks.
#[derive(Debug, Clone)]
pub(crate) enum SupervisedTaskMessage {
    /// Sent by tasks to indicate they are alive
    Heartbeat(Heartbeat),
    /// Sent by the supervisor to itself to trigger a task restart
    Restart(TaskName),
    Completed(TaskName, TaskOutcome),
}

/// Main component that handles all the tasks.
///
/// Spawn each tasks along with a `Heartbeat` thread tied to the main one,
/// responsible of proving that the main thread is alive.
/// If one threads stops, the other stops too.
pub struct Supervisor {
    pub(crate) tasks: HashMap<TaskName, TaskHandle>,
    pub(crate) timeout_treshold: Duration,
    pub(crate) external_tx: mpsc::UnboundedSender<SupervisorMessage>,
    pub(crate) external_rx: mpsc::UnboundedReceiver<SupervisorMessage>,
    pub(crate) tx: mpsc::UnboundedSender<SupervisedTaskMessage>,
    pub(crate) rx: mpsc::UnboundedReceiver<SupervisedTaskMessage>,
}

impl Supervisor {
    /// Runs the supervisor, consuming it and returning a handle for control.
    pub fn run(self) -> SupervisorHandle {
        let user_tx = self.external_tx.clone();
        let handle = tokio::spawn(async move {
            self.run_and_supervise().await;
        });
        SupervisorHandle::new(handle, user_tx)
    }

    /// Internal method to start and supervise all tasks.
    async fn run_and_supervise(mut self) {
        if self.tasks.is_empty() {
            return;
        }
        self.start_all_tasks().await;
        self.supervise_all_tasks().await;
    }

    async fn start_all_tasks(&mut self) {
        for (task_name, task_handle) in self.tasks.iter_mut() {
            Self::start_task(task_name.to_string(), task_handle, self.tx.clone()).await;
        }
    }

    async fn supervise_all_tasks(&mut self) {
        let mut health_check_interval = interval_at(
            tokio::time::Instant::now() + Duration::from_secs(3),
            Duration::from_secs(1),
        );

        loop {
            tokio::select! {
                // Handle internal messages (heartbeats, restarts)
                Some(internal_msg) = self.rx.recv() => {
                    match internal_msg {
                        SupervisedTaskMessage::Heartbeat(heartbeat) => {
                            self.register_heartbeat(heartbeat);
                        },
                        SupervisedTaskMessage::Restart(task_name) => {
                            self.restart_task(task_name).await;
                        },
                        SupervisedTaskMessage::Completed(task_name, outcome) => {
                            self.handle_task_completion(task_name, outcome).await;
                        }
                    }
                },

                // Handle user messages through Supervisor handle
                Some(user_msg) = self.external_rx.recv() => {
                    self.handle_user_message(user_msg).await;
                },

                // Periodic health check
                _ = health_check_interval.tick() => {
                    self.check_all_health();
                    if self.all_tasks_died() {
                        break;
                    }
                }
            }
        }
    }

    async fn handle_user_message(&mut self, msg: SupervisorMessage) {
        match msg {
            SupervisorMessage::AddTask(task_name, task) => {
                // TODO: See what to do. Ignore for now. Should probably return
                // an error to the User.
                if self.tasks.contains_key(&task_name) {
                    return;
                }

                let mut task_handle = TaskHandle::from_dyn_task(task);
                Self::start_task(task_name.clone(), &mut task_handle, self.tx.clone()).await;
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

                task_handle.clean_before_restart();
                task_handle.mark(TaskStatus::Dead);
            }

            SupervisorMessage::Shutdown => {
                for (_task_name, task_handle) in self.tasks.iter_mut() {
                    if task_handle.status != TaskStatus::Dead {
                        task_handle.clean_before_restart();
                        task_handle.mark(TaskStatus::Dead);
                    }
                }
            }
        }
    }

    async fn start_task(
        task_name: TaskName,
        task_handle: &mut TaskHandle,
        tx: mpsc::UnboundedSender<SupervisedTaskMessage>,
    ) {
        let mut task = task_handle.task.clone_task();
        let task_name_c = task_name.clone();

        // Create a channel to communicate task completion status
        let (completion_tx, mut completion_rx) = mpsc::channel::<TaskOutcome>(1);

        let handle = tokio::spawn(async move {
            let completion_tx_clone = completion_tx.clone();

            let ran_task = tokio::spawn(async move {
                match task.run().await {
                    Ok(outcome) => {
                        // Successfully send back the completion status
                        let _ = completion_tx_clone.send(outcome).await;
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            });

            let task_name_a = task_name_c.clone();
            let tx_a = tx.clone();
            let heartbeat_task = tokio::spawn(async move {
                let mut beat_interval = tokio::time::interval(Duration::from_millis(500));
                loop {
                    beat_interval.tick().await;
                    let beat = SupervisedTaskMessage::Heartbeat(Heartbeat::new(&task_name_a));
                    if tx_a.send(beat).is_err() {
                        break;
                    }
                }
                Ok(())
            });

            // Add the completion receiver to abort tasks appropriately
            let completion_task = tokio::spawn(async move {
                if let Some(outcome) = completion_rx.recv().await {
                    // Send completion notification to supervisor
                    let completion_msg = SupervisedTaskMessage::Completed(task_name_c, outcome);
                    let _ = tx.send(completion_msg);
                }
                Ok(())
            });

            TaskGroup::new()
                .with_handle(ran_task)
                .with_handle(heartbeat_task)
                .with_handle(completion_task)
                .abort_all_if_one_resolves()
                .await
        });

        task_handle.handle = Some(handle);
        task_handle.mark(TaskStatus::Starting);
    }

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
                if let Some(healthy_since) = task_handle.healthy_since {
                    const TASK_IS_STABLE_DURATION: Duration = Duration::from_secs(600);
                    if heartbeat.timestamp.duration_since(healthy_since) > TASK_IS_STABLE_DURATION {
                        task_handle.restart_attempts = 0;
                    }
                } else {
                    task_handle.healthy_since = Some(heartbeat.timestamp);
                }
            }
            _ => {}
        }
    }

    async fn restart_task(&mut self, task_name: TaskName) {
        let Some(task_handle) = self.tasks.get_mut(&task_name) else {
            return;
        };
        task_handle.clean_before_restart();
        Self::start_task(task_name, task_handle, self.tx.clone()).await;
    }

    fn check_all_health(&mut self) {
        let crashed_tasks = self
            .tasks
            .iter()
            .filter(|(_, handle)| handle.has_crashed(self.timeout_treshold))
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();

        for crashed_task in crashed_tasks {
            let Some(task_handle) = self.tasks.get_mut(&crashed_task) else {
                return;
            };
            if task_handle.has_exceeded_max_retries() && task_handle.status != TaskStatus::Dead {
                task_handle.mark(TaskStatus::Dead);
                continue;
            }
            let restart_delay = task_handle.restart_delay();
            task_handle.mark(TaskStatus::Failed);
            task_handle.restart_attempts = task_handle.restart_attempts.saturating_add(1);
            let tx = self.tx.clone();
            tokio::spawn(async move {
                tokio::time::sleep(restart_delay).await;
                let _ = tx.send(SupervisedTaskMessage::Restart(crashed_task));
            });
        }
    }

    fn all_tasks_died(&self) -> bool {
        !self.tasks.is_empty()
            && self
                .tasks
                .values()
                .all(|handle| handle.status == TaskStatus::Dead)
    }

    async fn handle_task_completion(&mut self, task_name: TaskName, outcome: TaskOutcome) {
        let Some(task_handle) = self.tasks.get_mut(&task_name) else {
            return;
        };

        match outcome {
            TaskOutcome::Completed => {
                // Task successfully completed - mark as completed and don't restart
                task_handle.mark(TaskStatus::Completed);
                // Clean up resources but don't schedule restart
                task_handle.clean_before_restart();
            }
            TaskOutcome::Failed(_reason) => {
                // Controlled failure - mark as failed and restart with backoff
                task_handle.mark(TaskStatus::Failed);
                if task_handle.has_exceeded_max_retries() {
                    task_handle.mark(TaskStatus::Dead);
                    return;
                }

                let restart_delay = task_handle.restart_delay();
                task_handle.restart_attempts = task_handle.restart_attempts.saturating_add(1);

                let tx = self.tx.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(restart_delay).await;
                    let _ = tx.send(SupervisedTaskMessage::Restart(task_name));
                });
            }
        }
    }
}
