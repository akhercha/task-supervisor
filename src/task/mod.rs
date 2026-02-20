use std::{
    future::Future,
    pin::Pin,
    time::{Duration, Instant},
};

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub type TaskError = anyhow::Error;
pub type TaskResult = Result<(), TaskError>;

/// The trait users implement for tasks managed by the supervisor.
///
/// # Example
/// ```rust
/// use task_supervisor::{SupervisedTask, TaskResult};
///
/// #[derive(Clone)]
/// struct MyTask;
///
/// impl SupervisedTask for MyTask {
///     async fn run(&mut self) -> TaskResult {
///         // do work...
///         Ok(())
///     }
/// }
/// ```
pub trait SupervisedTask: Send + 'static {
    /// Runs the task until completion or failure.
    fn run(&mut self) -> impl Future<Output = TaskResult> + Send;
}

// ---- Internal dyn-compatible wrapper ----

/// Object-safe version of `SupervisedTask` used internally for dynamic dispatch.
/// Users never see or implement this trait directly.
pub(crate) trait DynSupervisedTask: Send + 'static {
    fn run_boxed(&mut self) -> Pin<Box<dyn Future<Output = TaskResult> + Send + '_>>;
    fn clone_box(&self) -> Box<dyn DynSupervisedTask>;
}

impl<T> DynSupervisedTask for T
where
    T: SupervisedTask + Clone + Send + 'static,
{
    fn run_boxed(&mut self) -> Pin<Box<dyn Future<Output = TaskResult> + Send + '_>> {
        Box::pin(self.run())
    }

    fn clone_box(&self) -> Box<dyn DynSupervisedTask> {
        Box::new(self.clone())
    }
}

pub(crate) type DynTask = Box<dyn DynSupervisedTask>;

/// Represents the current state of a supervised task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Task has been created but not yet started.
    Created,
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
            Self::Healthy => write!(f, "healthy"),
            Self::Failed => write!(f, "failed"),
            Self::Completed => write!(f, "completed"),
            Self::Dead => write!(f, "dead"),
        }
    }
}

pub(crate) struct TaskHandle {
    pub(crate) status: TaskStatus,
    pub(crate) task: DynTask,
    pub(crate) join_handle: Option<JoinHandle<()>>,
    pub(crate) restart_attempts: u32,
    pub(crate) healthy_since: Option<Instant>,
    pub(crate) cancellation_token: Option<CancellationToken>,
    pub(crate) max_restart_attempts: Option<u32>,
    pub(crate) base_restart_delay: Duration,
    pub(crate) max_backoff_exponent: u32,
}

impl TaskHandle {
    /// Creates a `TaskHandle` from a boxed task.
    /// Configuration is set later by the builder at `build()` time.
    pub(crate) fn new(task: DynTask) -> Self {
        Self {
            status: TaskStatus::Created,
            task,
            join_handle: None,
            restart_attempts: 0,
            healthy_since: None,
            cancellation_token: None,
            max_restart_attempts: None,
            base_restart_delay: Duration::from_secs(1),
            max_backoff_exponent: 5,
        }
    }

    /// Creates a new `TaskHandle` from a concrete task type.
    pub(crate) fn from_task<T: SupervisedTask + Clone>(task: T) -> Self {
        Self::new(Box::new(task))
    }

    /// Calculates the restart delay using exponential backoff.
    pub(crate) fn restart_delay(&self) -> Duration {
        let factor = 2u32.saturating_pow(self.restart_attempts.min(self.max_backoff_exponent));
        self.base_restart_delay.saturating_mul(factor)
    }

    /// Checks if the task has exceeded its maximum restart attempts.
    pub(crate) const fn has_exceeded_max_retries(&self) -> bool {
        if let Some(max_restart_attempts) = self.max_restart_attempts {
            self.restart_attempts >= max_restart_attempts
        } else {
            false
        }
    }

    /// Updates the task's status.
    pub(crate) fn mark(&mut self, status: TaskStatus) {
        self.status = status;
    }

    /// Cleans up the task by cancelling its token and aborting the join handle.
    pub(crate) fn clean(&mut self) {
        if let Some(token) = self.cancellation_token.take() {
            token.cancel();
        }
        if let Some(handle) = self.join_handle.take() {
            handle.abort();
        }
        self.healthy_since = None;
    }
}
