use std::{
    error::Error,
    time::{Duration, Instant},
};

use tokio_util::sync::CancellationToken;

pub type DynTask = Box<dyn CloneableSupervisedTask>;
pub type TaskError = Box<dyn Error + Send + Sync>;

#[async_trait::async_trait]
pub trait SupervisedTask: Send + 'static {
    /// Runs the task until completion or failure.
    async fn run(&mut self) -> Result<TaskOutcome, TaskError>;
}

pub trait CloneableSupervisedTask: SupervisedTask {
    fn clone_box(&self) -> Box<dyn CloneableSupervisedTask>;
}

impl<T> CloneableSupervisedTask for T
where
    T: SupervisedTask + Clone + Send + 'static,
{
    fn clone_box(&self) -> Box<dyn CloneableSupervisedTask> {
        Box::new(self.clone())
    }
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

impl TaskStatus {
    pub fn is_restarting(&self) -> bool {
        matches!(self, TaskStatus::Failed)
    }

    pub fn is_healthy(&self) -> bool {
        matches!(self, TaskStatus::Healthy)
    }

    pub fn is_dead(&self) -> bool {
        matches!(self, TaskStatus::Dead)
    }

    pub fn has_completed(&self) -> bool {
        matches!(self, TaskStatus::Completed)
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Created => write!(f, "created"),
            Self::Starting => write!(f, "starting"),
            Self::Healthy => write!(f, "healthy"),
            Self::Failed => write!(f, "failed"),
            Self::Completed => write!(f, "completed"),
            Self::Dead => write!(f, "dead"),
        }
    }
}

/// Outcome of a task's execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskOutcome {
    /// Task completed successfully and should not be restarted.
    Completed,
    /// Task failed and may be restarted, with an optional reason.
    Failed(String),
}

impl std::fmt::Display for TaskOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Completed => write!(f, "completed"),
            Self::Failed(e) => write!(f, "failed: {e}"),
        }
    }
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
    /// Creates a `TaskHandle` from a boxed task with default configuration.
    pub(crate) fn new(
        task: Box<dyn CloneableSupervisedTask>,
        max_restart_attempts: u32,
        base_restart_delay: Duration,
    ) -> Self {
        Self {
            status: TaskStatus::Created,
            task,
            handles: None,
            last_heartbeat: None,
            restart_attempts: 0,
            healthy_since: None,
            cancellation_token: None,
            max_restart_attempts,
            base_restart_delay,
        }
    }

    /// Creates a new `TaskHandle` with custom restart configuration.
    pub(crate) fn from_task<T: CloneableSupervisedTask + 'static>(
        task: T,
        max_restart_attempts: u32,
        base_restart_delay: Duration,
    ) -> Self {
        let task = Box::new(task);
        Self::new(task, max_restart_attempts, base_restart_delay)
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
