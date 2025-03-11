use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

pub type DynTask = Box<dyn SupervisedTask>;
pub type TaskError = Box<dyn std::error::Error + Send + Sync>;

#[async_trait::async_trait]
pub trait SupervisedTask: Send + 'static {
    /// Runs the task until completion or failure.
    async fn run(&mut self) -> Result<TaskOutcome, TaskError>;

    /// Clones the task into a boxed trait object.
    fn clone_task(&self) -> Box<dyn SupervisedTask>;
}

/// Represents the current state of a supervised task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Task has been created but not yet started.
    Created,
    /// Task is in the process of starting.
    Starting,
    /// Task is running and healthy.
    Healthy,
    /// Task has failed and is pending restart.
    Failed,
    /// Task has completed successfully.
    Completed,
    /// Task has failed too many times and is terminated.
    Dead,
}

/// Outcome of a task's execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskOutcome {
    /// Task completed successfully and should not be restarted.
    Completed,
    /// Task failed and may be restarted, with an optional reason.
    Failed(String),
}

pub(crate) struct TaskHandle {
    pub(crate) status: TaskStatus,
    pub(crate) task: DynTask,
    pub(crate) handles: Option<Vec<tokio::task::JoinHandle<()>>>,
    pub(crate) last_heartbeat: Option<Instant>,
    pub(crate) restart_attempts: u32,
    pub(crate) healthy_since: Option<Instant>,
    pub(crate) cancellation_token: Option<CancellationToken>,
    max_restart_attempts: u32,
    base_restart_delay: Duration,
}

impl TaskHandle {
    /// Creates a new `TaskHandle` with custom restart configuration.
    pub(crate) fn new_with_config<T: SupervisedTask + 'static>(
        task: T,
        max_restart_attempts: u32,
        base_restart_delay: Duration,
    ) -> Self {
        Self {
            status: TaskStatus::Created,
            task: Box::new(task),
            handles: None,
            last_heartbeat: None,
            restart_attempts: 0,
            healthy_since: None,
            cancellation_token: None,
            max_restart_attempts,
            base_restart_delay,
        }
    }

    /// Creates a `TaskHandle` from a boxed task with default configuration.
    pub(crate) fn from_dyn_task(task: Box<dyn SupervisedTask>) -> Self {
        const MAX_RESTART_ATTEMPS: u32 = 5;
        const BASE_RESTART_DELAY: Duration = Duration::from_secs(3);
        Self {
            status: TaskStatus::Created,
            task,
            handles: None,
            last_heartbeat: None,
            restart_attempts: 0,
            healthy_since: None,
            cancellation_token: None,
            max_restart_attempts: MAX_RESTART_ATTEMPS,
            base_restart_delay: BASE_RESTART_DELAY,
        }
    }

    /// Updates the last heartbeat time.
    pub(crate) fn ticked_at(&mut self, at: Instant) {
        self.last_heartbeat = Some(at);
    }

    /// Calculates the time since the last heartbeat.
    pub(crate) fn time_since_last_heartbeat(&self) -> Option<Duration> {
        self.last_heartbeat
            .map(|last| Instant::now().duration_since(last))
    }

    /// Checks if the task has crashed based on the timeout threshold.
    pub(crate) fn has_crashed(&self, timeout_threshold: Duration) -> bool {
        let Some(time_since_last_heartbeat) = self.time_since_last_heartbeat() else {
            return !self.is_ko();
        };
        !self.is_ko() && time_since_last_heartbeat > timeout_threshold
    }

    /// Calculates the restart delay using exponential backoff.
    pub(crate) fn restart_delay(&self) -> Duration {
        let factor = 2u32.saturating_pow(self.restart_attempts.min(5));
        self.base_restart_delay.saturating_mul(factor)
    }

    /// Checks if the task has exceeded its maximum restart attempts.
    pub(crate) const fn has_exceeded_max_retries(&self) -> bool {
        self.restart_attempts >= self.max_restart_attempts
    }

    /// Updates the task's status.
    pub(crate) fn mark(&mut self, status: TaskStatus) {
        self.status = status;
    }

    /// Cleans up the task by aborting its handle and resetting state.
    pub(crate) async fn clean(&mut self) {
        if let Some(token) = self.cancellation_token.take() {
            token.cancel();
        }
        self.last_heartbeat = None;
        self.healthy_since = None;
        if let Some(handles) = self.handles.take() {
            for handle in handles {
                handle.abort();
            }
        }
    }

    /// Checks if the task is in a failed or dead state.
    pub(crate) fn is_ko(&self) -> bool {
        self.status == TaskStatus::Failed || self.status == TaskStatus::Dead
    }
}
