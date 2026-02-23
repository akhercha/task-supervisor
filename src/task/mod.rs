use std::{
    future::Future,
    pin::Pin,
    time::{Duration, Instant},
};

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

#[cfg(feature = "anyhow")]
pub type TaskError = anyhow::Error;
#[cfg(not(feature = "anyhow"))]
pub type TaskError = Box<dyn std::error::Error + Send + Sync + 'static>;

pub type TaskResult = Result<(), TaskError>;

/// The trait users implement for tasks managed by the supervisor.
///
/// # Clone and restart semantics
///
/// The supervisor stores the **original** instance and clones it for each
/// run. Mutations via `&mut self` only live in the clone and are lost on
/// restart. Shared state (`Arc<...>`) survives because `Clone` just bumps
/// the refcount.
///
/// Use owned fields for per-run state, `Arc` for cross-restart state.
///
/// # Example
///
/// ```rust
/// use task_supervisor::{SupervisedTask, TaskResult};
/// use std::sync::Arc;
/// use std::sync::atomic::{AtomicUsize, Ordering};
///
/// #[derive(Clone)]
/// struct MyTask {
///     /// Reset to 0 on every restart (owned, cloned from original).
///     local_counter: u64,
///     /// Shared across restarts (Arc, cloned by reference).
///     total_runs: Arc<AtomicUsize>,
/// }
///
/// impl SupervisedTask for MyTask {
///     async fn run(&mut self) -> TaskResult {
///         self.total_runs.fetch_add(1, Ordering::Relaxed);
///         self.local_counter += 1;
///         // local_counter is always 1 here — fresh clone each restart.
///         Ok(())
///     }
/// }
/// ```
pub trait SupervisedTask: Send + 'static {
    /// Runs the task until completion or failure.
    ///
    /// Mutations to `&mut self` are **not** preserved across restarts.
    /// See the [trait-level docs](SupervisedTask) for details.
    fn run(&mut self) -> impl Future<Output = TaskResult> + Send;
}

/// Dyn-compatible wrapper for `SupervisedTask`. Not user-facing.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Created,
    Healthy,
    Failed,
    Completed,
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
    pub(crate) fn new(task: DynTask) -> Self {
        Self {
            status: TaskStatus::Created,
            task,
            join_handle: None,
            restart_attempts: 0,
            healthy_since: None,
            cancellation_token: None,
            // Defaults — overwritten by the builder at build() time.
            max_restart_attempts: None,
            base_restart_delay: Duration::from_secs(1),
            max_backoff_exponent: 5,
        }
    }

    pub(crate) fn from_task<T: SupervisedTask + Clone>(task: T) -> Self {
        Self::new(Box::new(task))
    }

    /// Delay = base_restart_delay * 2^min(attempts, max_backoff_exponent).
    pub(crate) fn restart_delay(&self) -> Duration {
        let factor = 2u32.saturating_pow(self.restart_attempts.min(self.max_backoff_exponent));
        self.base_restart_delay.saturating_mul(factor)
    }

    pub(crate) const fn has_exceeded_max_retries(&self) -> bool {
        if let Some(max_restart_attempts) = self.max_restart_attempts {
            self.restart_attempts >= max_restart_attempts
        } else {
            false
        }
    }

    pub(crate) fn mark(&mut self, status: TaskStatus) {
        self.status = status;
    }

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
