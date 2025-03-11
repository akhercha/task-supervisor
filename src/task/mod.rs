use std::time::{Duration, Instant};

pub type DynTask = Box<dyn SupervisedTask>;

#[async_trait::async_trait]
pub trait SupervisedTask: Send + 'static {
    /// Run the task until completion or failure
    async fn run(&mut self) -> Result<TaskOutcome, Box<dyn std::error::Error + Send + Sync>>;

    /// Clone the current task into a Box.
    fn clone_task(&self) -> Box<dyn SupervisedTask>;
}

/// Status of a task
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Task has just been created and will be starting soon
    Created,
    /// Task is starting up
    Starting,
    /// Task is running normally
    Healthy,
    /// Task failed and will be restarted
    Failed,
    /// Task successfully completed its work
    Completed,
    /// Task has exceeded max retries & we stopped trying
    Dead,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskOutcome {
    /// Task completed its work successfully and should not be restarted
    Completed,
    /// Task encountered an error but should be restarted
    Failed(String), // Optional error context
}

pub(crate) struct TaskHandle {
    pub(crate) status: TaskStatus,
    pub(crate) task: DynTask,
    pub(crate) handle: Option<tokio::task::JoinHandle<()>>,
    pub(crate) last_heartbeat: Option<Instant>,
    pub(crate) restart_attempts: u32,
    pub(crate) healthy_since: Option<Instant>,
    max_restart_attempts: u32,
    base_restart_delay: Duration,
}

impl TaskHandle {
    pub(crate) fn new<T: SupervisedTask + 'static>(task: T) -> Self {
        Self {
            status: TaskStatus::Created,
            task: Box::new(task),
            handle: None,
            last_heartbeat: None,
            restart_attempts: 0,
            healthy_since: None,
            max_restart_attempts: 5,
            base_restart_delay: Duration::from_secs(1),
        }
    }

    pub(crate) fn from_dyn_task(task: Box<dyn SupervisedTask>) -> Self {
        Self {
            status: TaskStatus::Created,
            task,
            handle: None,
            last_heartbeat: None,
            restart_attempts: 0,
            healthy_since: None,
            max_restart_attempts: 5,
            base_restart_delay: Duration::from_secs(1),
        }
    }

    pub(crate) fn ticked_at(&mut self, at: Instant) {
        self.last_heartbeat = Some(at);
    }

    pub(crate) fn time_since_last_heartbeat(&self) -> Option<Duration> {
        self.last_heartbeat
            .map(|last_heartbeat| Instant::now().duration_since(last_heartbeat))
    }

    pub(crate) fn has_crashed(&self, timeout_threshold: Duration) -> bool {
        let Some(time_since_last_heartbeat) = self.time_since_last_heartbeat() else {
            return !self.is_ko();
        };
        (!self.is_ko()) && (time_since_last_heartbeat > timeout_threshold)
    }

    pub(crate) fn restart_delay(&self) -> Duration {
        let factor = 2u32.saturating_pow(self.restart_attempts.min(5));
        self.base_restart_delay.saturating_mul(factor)
    }

    pub(crate) const fn has_exceeded_max_retries(&self) -> bool {
        self.restart_attempts >= self.max_restart_attempts
    }

    pub(crate) fn mark(&mut self, status: TaskStatus) {
        self.status = status;
    }

    pub(crate) async fn clean(&mut self) {
        self.last_heartbeat = None;
        self.healthy_since = None;
        if let Some(still_running_task) = self.handle.take() {
            still_running_task.abort();
            assert!(still_running_task.await.unwrap_err().is_cancelled());
        }
    }

    pub(crate) fn is_ko(&self) -> bool {
        (self.status == TaskStatus::Failed) || (self.status == TaskStatus::Dead)
    }
}
