pub(crate) mod builder;
pub(crate) mod handle;

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use handle::SupervisorMessage;
use tokio::{sync::mpsc, time::interval_at};
use uuid::Uuid;

use crate::{
    supervisor::handle::SupervisorHandle,
    task::{SupervisedTask, TaskHandle, TaskStatus},
    utils::TaskGroup,
    TaskId,
};

/// A beat sent from a task to indicate that it's alive.
#[derive(Debug, Clone)]
pub(crate) struct Heartbeat {
    pub(crate) task_id: TaskId,
    pub(crate) timestamp: Instant,
}

impl Heartbeat {
    pub fn new(task_id: TaskId) -> Self {
        Self {
            task_id,
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
    Restart(TaskId),
}

/// Main component that handles all the tasks.
///
/// Spawn each tasks along with a `Heartbeat` thread tied to the main one,
/// responsible of proving that the main thread is alive.
/// If one threads stops, the other stops too.
pub struct Supervisor<T: SupervisedTask> {
    pub(crate) tasks: HashMap<TaskId, TaskHandle<T>>,
    pub(crate) timeout_treshold: Duration,
    pub(crate) external_tx: mpsc::UnboundedSender<SupervisorMessage<T>>,
    pub(crate) external_rx: mpsc::UnboundedReceiver<SupervisorMessage<T>>,
    pub(crate) tx: mpsc::UnboundedSender<SupervisedTaskMessage>,
    pub(crate) rx: mpsc::UnboundedReceiver<SupervisedTaskMessage>,
}

impl<T> Supervisor<T>
where
    T: SupervisedTask + Clone + Send + 'static,
{
    /// Runs the supervisor, consuming it and returning a handle for control.
    pub fn run(self) -> SupervisorHandle<T> {
        let user_tx = self.external_tx.clone();
        let handle = tokio::spawn(async move {
            self.run_and_supervise().await;
        });
        SupervisorHandle {
            tx: user_tx,
            join_handle: handle,
        }
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
        for (task_id, task_handle) in self.tasks.iter_mut() {
            Self::start_task(*task_id, task_handle, self.tx.clone()).await;
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
                        SupervisedTaskMessage::Restart(task_id) => {
                            self.restart_task(task_id).await;
                        }
                    }
                },

                // Handle user messages through Supervisor handle
                Some(user_msg) = self.external_rx.recv() => {
                    match user_msg {
                        SupervisorMessage::AddTask(task) => {
                            let task_id = Uuid::new_v4();
                            let mut task_handle = TaskHandle::new(task);
                            Self::start_task(task_id, &mut task_handle, self.tx.clone()).await;
                            self.tasks.insert(task_id, task_handle);
                        }
                        _ => todo!("Not implemented yet!")
                    }
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

    async fn start_task(
        task_id: TaskId,
        task_handle: &mut TaskHandle<T>,
        tx: mpsc::UnboundedSender<SupervisedTaskMessage>,
    ) {
        let mut task = task_handle.task.clone();
        let handle = tokio::spawn(async move {
            let ran_task = tokio::spawn(async move { task.run_forever().await });
            let heartbeat_task = tokio::spawn(async move {
                let mut beat_interval = tokio::time::interval(Duration::from_millis(500));
                loop {
                    beat_interval.tick().await;
                    let beat = SupervisedTaskMessage::Heartbeat(Heartbeat::new(task_id));
                    if tx.send(beat).is_err() {
                        break;
                    }
                }
                Ok(())
            });
            TaskGroup::new()
                .with_handle(ran_task)
                .with_handle(heartbeat_task)
                // TODO: This is not always good.
                // We may want to a run task and if it resolves it's actually
                // intended? And we don't want to restart it?
                .abort_all_if_one_resolves()
                .await
        });
        task_handle.handle = Some(handle);
        task_handle.mark(TaskStatus::Starting);
    }

    fn register_heartbeat(&mut self, heartbeat: Heartbeat) {
        let Some(task_handle) = self.tasks.get_mut(&heartbeat.task_id) else {
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

    async fn restart_task(&mut self, task_id: TaskId) {
        let Some(task_handle) = self.tasks.get_mut(&task_id) else {
            return;
        };
        task_handle.clean_before_restart();
        Self::start_task(task_id, task_handle, self.tx.clone()).await;
    }

    fn check_all_health(&mut self) {
        let crashed_tasks = self
            .tasks
            .iter()
            .filter(|(_, handle)| handle.has_crashed(self.timeout_treshold))
            .map(|(id, _)| *id)
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
}
