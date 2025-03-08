use std::{collections::HashMap, time::Duration};

use tokio::{
    sync::mpsc,
    time::{interval_at, Instant},
};
use uuid::Uuid;

use crate::{
    messaging::{Heartbeat, SupervisorMessage},
    task::{SupervisedTask, TaskHandle, TaskStatus},
    task_group::TaskGroup,
};

pub type TaskId = uuid::Uuid;

pub struct Supervisor<T: SupervisedTask> {
    tasks: HashMap<TaskId, TaskHandle<T>>,
    timeout_treshold: Duration,
    rx: mpsc::UnboundedReceiver<SupervisorMessage>,
    tx: mpsc::UnboundedSender<SupervisorMessage>,
}

impl<T> Supervisor<T>
where
    T: SupervisedTask + Clone + Send + 'static,
{
    pub async fn run_and_supervise(&mut self) {
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
            Instant::now() + Duration::from_secs(3),
            Duration::from_secs(1),
        );

        loop {
            tokio::select! {
                Some(msg) = self.rx.recv() => {
                    match msg {
                        SupervisorMessage::Heartbeat(heartbeat) => {
                            self.register_heartbeat(heartbeat);
                        },
                        SupervisorMessage::Restart(task_id) => {
                            self.restart_task(task_id).await;
                        }
                    }
                },

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
        tx: mpsc::UnboundedSender<SupervisorMessage>,
    ) {
        let mut task = task_handle.task.clone();

        let handle = tokio::spawn(async move {
            // Run the provided task
            let ran_task = tokio::spawn(async move { task.run_forever().await });

            // Beat every seconds
            let heartbeat_task = tokio::spawn(async move {
                let mut beat_interval = tokio::time::interval(Duration::from_millis(500));
                loop {
                    beat_interval.tick().await;
                    let beat = SupervisorMessage::Heartbeat(Heartbeat::new(task_id));
                    if tx.send(beat).is_err() {
                        break;
                    }
                }
                Ok(())
            });

            TaskGroup::new()
                .with_handle(ran_task)
                .with_handle(heartbeat_task)
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
                let _ = tx.send(SupervisorMessage::Restart(crashed_task));
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

// TODO: We should be able to configure:
// * max_restarts,
// * base_restart_delay,
#[derive(Debug)]
pub struct SupervisorBuilder<T: SupervisedTask> {
    tasks: HashMap<TaskId, TaskHandle<T>>,
}

impl<T: SupervisedTask> SupervisorBuilder<T> {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
        }
    }

    pub fn with_task(mut self, task: T) -> Self {
        self.tasks.insert(Uuid::new_v4(), TaskHandle::new(task));
        self
    }

    pub fn with_tasks<I>(mut self, tasks: I) -> Self
    where
        I: IntoIterator<Item = T>,
    {
        for task in tasks {
            self = self.with_task(task);
        }
        self
    }

    pub fn build(self) -> Supervisor<T> {
        let (tx, rx) = mpsc::unbounded_channel();

        Supervisor {
            tasks: self.tasks,
            timeout_treshold: Duration::from_secs(2),
            tx,
            rx,
        }
    }
}

impl<T: SupervisedTask> Default for SupervisorBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}
