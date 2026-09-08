use std::{collections::VecDeque, future::Future, pin::Pin, sync::Arc};

use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::handle::SupervisorHandleError;

/// Error type returned by [`SupervisedTask::run`].
///
/// Any `std::error::Error + Send + Sync` converts into it with `?`, including
/// `anyhow::Error`.
pub type TaskError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Outcome of one run of a [`SupervisedTask`].
pub type TaskResult = Result<(), TaskError>;

/// A long-lived unit of work managed by the supervisor.
///
/// # Lifecycle
///
/// Every start or restart clones the instance registered with the supervisor
/// and hands the clone to `run` by value. Owned fields therefore start from
/// their registered value on every run; `Arc` fields are shared across runs.
///
/// | `run` outcome | Supervisor reaction |
/// | --- | --- |
/// | `Ok(())` | Task is [`Completed`](TaskStatus::Completed); never restarted automatically |
/// | `Err(_)` or panic | Restarted after an exponential backoff, until the restart budget is exhausted |
///
/// # Cancellation
///
/// `cancel` is triggered when the supervisor wants the task to stop (kill,
/// restart, shutdown). The task then has `stop_timeout` to return on its
/// own; after that its future is dropped. Tasks that do not need cleanup can
/// ignore the token.
///
/// # Example
///
/// ```rust
/// use std::sync::Arc;
/// use std::sync::atomic::{AtomicUsize, Ordering};
/// use task_supervisor::{CancellationToken, SupervisedTask, TaskResult};
///
/// #[derive(Clone)]
/// struct Worker {
///     /// Reset on every run (owned, cloned from the original).
///     polls: u64,
///     /// Shared across runs (`Arc`, cloned by reference).
///     total_polls: Arc<AtomicUsize>,
/// }
///
/// impl SupervisedTask for Worker {
///     async fn run(mut self, cancel: CancellationToken) -> TaskResult {
///         loop {
///             tokio::select! {
///                 _ = cancel.cancelled() => return Ok(()),
///                 _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
///                     self.polls += 1;
///                     self.total_polls.fetch_add(1, Ordering::Relaxed);
///                 }
///             }
///         }
///     }
/// }
/// ```
pub trait SupervisedTask: Clone + Send + 'static {
    /// Runs one instance of the task until it completes, fails, or is cancelled.
    fn run(self, cancel: CancellationToken) -> impl Future<Output = TaskResult> + Send;
}

/// Object-safe view of [`SupervisedTask`].
pub(crate) trait DynTask: Send + 'static {
    fn run(
        self: Box<Self>,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = TaskResult> + Send>>;
    fn clone_box(&self) -> Box<dyn DynTask>;
}

impl<T: SupervisedTask> DynTask for T {
    fn run(
        self: Box<Self>,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = TaskResult> + Send>> {
        Box::pin(SupervisedTask::run(*self, cancel))
    }

    fn clone_box(&self) -> Box<dyn DynTask> {
        Box::new(self.clone())
    }
}

/// Observable state of a supervised task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskStatus {
    /// `run` is executing.
    Running,
    /// `run` failed; a restart is scheduled after the backoff delay.
    Restarting,
    /// Cancellation was requested; waiting for `run` to return
    /// (bounded by `stop_timeout`).
    Stopping,
    /// `run` returned `Ok(())`. Terminal unless restarted manually.
    Completed,
    /// Killed, or restart budget exhausted. Terminal unless restarted manually.
    Dead,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Running => "running",
            Self::Restarting => "restarting",
            Self::Stopping => "stopping",
            Self::Completed => "completed",
            Self::Dead => "dead",
        })
    }
}

pub(crate) type Reply<T> = oneshot::Sender<Result<T, SupervisorHandleError>>;

/// Supervisor-side bookkeeping for one task. At most one run is alive per slot.
pub(crate) struct Slot {
    pub(crate) name: Arc<str>,
    pub(crate) status: TaskStatus,
    pub(crate) task: Box<dyn DynTask>,
    pub(crate) cancel: Option<CancellationToken>,
    /// Incremented on every start; stale backoff timers carry an older value.
    pub(crate) generation: u64,
    /// Restart instants inside the current window, oldest first.
    pub(crate) restarts: VecDeque<tokio::time::Instant>,
    /// What to do once a `Stopping` run exits.
    pub(crate) restart_after_stop: bool,
    /// `kill_task` / `restart_task` callers answered once the run has exited.
    pub(crate) stop_waiters: Vec<Reply<()>>,
}

impl Slot {
    pub(crate) fn new(name: Arc<str>, task: Box<dyn DynTask>) -> Self {
        Self {
            name,
            status: TaskStatus::Dead,
            task,
            cancel: None,
            generation: 0,
            restarts: VecDeque::new(),
            restart_after_stop: false,
            stop_waiters: Vec::new(),
        }
    }

    pub(crate) fn request_stop(&mut self, restart_after: bool) {
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
        }
        self.status = TaskStatus::Stopping;
        self.restart_after_stop = restart_after;
    }
}

pub(crate) fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_owned())
}
