use std::{collections::HashMap, sync::Arc};

use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};

use crate::{
    supervisor::{Outcome, SupervisorError},
    task::{panic_message, DynTask, Reply, SupervisedTask, TaskStatus},
};

/// Why a [`SupervisorHandle`] request was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SupervisorHandleError {
    /// The supervisor has exited or is shutting down.
    #[error("supervisor is no longer running")]
    Closed,
    /// `add_task` was called with a name that is already registered.
    #[error("task '{0}' already exists")]
    TaskAlreadyExists(String),
    /// No task is registered under this name.
    #[error("task '{0}' not found")]
    TaskNotFound(String),
}

pub(crate) enum Message {
    AddTask {
        name: Arc<str>,
        task: Box<dyn DynTask>,
        reply: Reply<()>,
    },
    RestartTask {
        name: String,
        reply: Reply<()>,
    },
    KillTask {
        name: String,
        reply: Reply<()>,
    },
    TaskStatus {
        name: String,
        reply: Reply<TaskStatus>,
    },
    TaskStatuses {
        reply: oneshot::Sender<HashMap<Arc<str>, TaskStatus>>,
    },
    Shutdown,
}

/// Controls a running supervisor. Cheap to clone.
///
/// Dropping the last handle shuts the supervisor down gracefully.
#[derive(Clone)]
pub struct SupervisorHandle {
    tx: mpsc::UnboundedSender<Message>,
    outcome: watch::Receiver<Option<Outcome>>,
}

impl SupervisorHandle {
    pub(crate) fn new(join: JoinHandle<Outcome>, tx: mpsc::UnboundedSender<Message>) -> Self {
        let (outcome_tx, outcome) = watch::channel(None);
        tokio::spawn(async move {
            let outcome = match join.await {
                Ok(outcome) => outcome,
                Err(err) if err.is_panic() => {
                    Err(SupervisorError::Panicked(panic_message(err.into_panic())))
                }
                Err(_) => Err(SupervisorError::Aborted),
            };
            let _ = outcome_tx.send(Some(outcome));
        });
        Self { tx, outcome }
    }

    /// Registers and starts a task. Returns once it is
    /// [`Running`](TaskStatus::Running).
    pub async fn add_task(
        &self,
        name: &str,
        task: impl SupervisedTask,
    ) -> Result<(), SupervisorHandleError> {
        self.request(|reply| Message::AddTask {
            name: Arc::from(name),
            task: Box::new(task),
            reply,
        })
        .await?
    }

    /// Restarts a task with a fresh restart budget and returns once the new
    /// run is [`Running`](TaskStatus::Running).
    ///
    /// A running task is cancelled first and has `stop_timeout` to exit.
    pub async fn restart_task(&self, name: &str) -> Result<(), SupervisorHandleError> {
        self.request(|reply| Message::RestartTask {
            name: name.to_owned(),
            reply,
        })
        .await?
    }

    /// Stops a task permanently and returns once it is
    /// [`Dead`](TaskStatus::Dead), which takes at most `stop_timeout`.
    /// Idempotent.
    pub async fn kill_task(&self, name: &str) -> Result<(), SupervisorHandleError> {
        self.request(|reply| Message::KillTask {
            name: name.to_owned(),
            reply,
        })
        .await?
    }

    /// Current status of one task.
    pub async fn task_status(&self, name: &str) -> Result<TaskStatus, SupervisorHandleError> {
        self.request(|reply| Message::TaskStatus {
            name: name.to_owned(),
            reply,
        })
        .await?
    }

    /// Current status of every task, keyed by name.
    pub async fn task_statuses(
        &self,
    ) -> Result<HashMap<Arc<str>, TaskStatus>, SupervisorHandleError> {
        self.request(|reply| Message::TaskStatuses { reply }).await
    }

    /// Cancels every task, waits for them to exit (bounded by `stop_timeout`)
    /// and returns the supervisor outcome. Same as [`wait`](Self::wait) if the
    /// supervisor already exited.
    pub async fn shutdown(&self) -> Outcome {
        let _ = self.tx.send(Message::Shutdown);
        self.wait().await
    }

    /// Waits for the supervisor to exit. Can be awaited any number of times,
    /// from any clone.
    pub async fn wait(&self) -> Outcome {
        let mut outcome = self.outcome.clone();
        loop {
            if let Some(outcome) = outcome.borrow_and_update().clone() {
                return outcome;
            }
            if outcome.changed().await.is_err() {
                return Err(SupervisorError::Aborted);
            }
        }
    }

    async fn request<T>(
        &self,
        message: impl FnOnce(oneshot::Sender<T>) -> Message,
    ) -> Result<T, SupervisorHandleError> {
        let (reply, response) = oneshot::channel();
        self.tx
            .send(message(reply))
            .map_err(|_| SupervisorHandleError::Closed)?;
        response.await.map_err(|_| SupervisorHandleError::Closed)
    }
}

impl std::fmt::Debug for SupervisorHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SupervisorHandle")
            .field("running", &!self.tx.is_closed())
            .finish()
    }
}
