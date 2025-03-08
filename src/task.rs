use std::time::{Duration, Instant};

#[async_trait::async_trait]
pub trait SupervisedTask {
    type Error: Send;

    fn name(&self) -> Option<&str> {
        None
    }

    async fn run_forever(&mut self) -> Result<(), Self::Error>;
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
    /// Task has exceeded max retries & we stopped trying
    Dead,
}

#[derive(Debug)]
pub(crate) struct TaskHandle<T: SupervisedTask> {
    pub(crate) status: TaskStatus,
    pub(crate) task: T,
    pub(crate) handle: Option<tokio::task::JoinHandle<()>>,
    pub(crate) last_heartbeat: Option<Instant>,
    pub(crate) restart_attempts: u32,
    max_restart_attempts: u32,
    base_restart_delay: Duration,
}

impl<T: SupervisedTask> TaskHandle<T> {
    pub(crate) fn new(task: T) -> Self {
        Self {
            status: TaskStatus::Created,
            task,
            handle: None,
            last_heartbeat: None,
            restart_attempts: 0,
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
            return self.status != TaskStatus::Dead;
        };
        (self.status != TaskStatus::Dead) && (time_since_last_heartbeat > timeout_threshold)
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

    pub(crate) fn clean_before_restart(&mut self) {
        self.last_heartbeat = None;
        if let Some(still_running_task) = self.handle.take() {
            still_running_task.abort();
        }
    }
}
