use std::collections::HashMap;

use thiserror::Error;
use tokio::sync::{mpsc, oneshot, watch};

use crate::{task::DynTask, SupervisedTask, TaskName, TaskStatus};

use super::SupervisorError;

#[derive(Debug, Error)]
pub enum SupervisorHandleError {
    #[error("Failed to send message to supervisor: channel closed")]
    SendError,
    #[error("Failed to receive response from supervisor: {0}")]
    RecvError(#[from] tokio::sync::oneshot::error::RecvError),
}

pub(crate) enum SupervisorMessage {
    AddTask(TaskName, DynTask),
    RestartTask(TaskName),
    KillTask(TaskName),
    GetTaskStatus(TaskName, oneshot::Sender<Option<TaskStatus>>),
    GetAllTaskStatuses(oneshot::Sender<HashMap<TaskName, TaskStatus>>),
    Shutdown,
}

/// Handle to interact with a running `Supervisor`. Cheap to clone.
#[derive(Clone)]
pub struct SupervisorHandle {
    pub(crate) tx: mpsc::UnboundedSender<SupervisorMessage>,
    result_rx: watch::Receiver<Option<Result<(), SupervisorError>>>,
}

impl Drop for SupervisorHandle {
    fn drop(&mut self) {
        if self.is_channel_open() {
            let _ = self.shutdown();
        }
    }
}

impl SupervisorHandle {
    pub(crate) fn new(
        join_handle: tokio::task::JoinHandle<Result<(), SupervisorError>>,
        tx: mpsc::UnboundedSender<SupervisorMessage>,
    ) -> Self {
        let (result_tx, result_rx) = watch::channel(None);

        tokio::spawn(async move {
            let result = match join_handle.await {
                Ok(supervisor_result) => supervisor_result,
                Err(_join_error) => Ok(()),
            };
            let _ = result_tx.send(Some(result));
        });

        Self { tx, result_rx }
    }

    /// Waits for the supervisor to finish. Safe to call from multiple clones concurrently.
    pub async fn wait(&self) -> Result<(), SupervisorError> {
        let mut rx = self.result_rx.clone();
        loop {
            if let Some(result) = rx.borrow_and_update().clone() {
                return result;
            }
            if rx.changed().await.is_err() {
                return Ok(());
            }
        }
    }

    pub fn add_task<T: SupervisedTask + Clone>(
        &self,
        task_name: &str,
        task: T,
    ) -> Result<(), SupervisorHandleError> {
        self.tx
            .send(SupervisorMessage::AddTask(task_name.into(), Box::new(task)))
            .map_err(|_| SupervisorHandleError::SendError)
    }

    pub fn restart(&self, task_name: &str) -> Result<(), SupervisorHandleError> {
        self.tx
            .send(SupervisorMessage::RestartTask(task_name.into()))
            .map_err(|_| SupervisorHandleError::SendError)
    }

    pub fn kill_task(&self, task_name: &str) -> Result<(), SupervisorHandleError> {
        self.tx
            .send(SupervisorMessage::KillTask(task_name.into()))
            .map_err(|_| SupervisorHandleError::SendError)
    }

    pub fn shutdown(&self) -> Result<(), SupervisorHandleError> {
        self.tx
            .send(SupervisorMessage::Shutdown)
            .map_err(|_| SupervisorHandleError::SendError)
    }

    pub async fn get_task_status(
        &self,
        task_name: &str,
    ) -> Result<Option<TaskStatus>, SupervisorHandleError> {
        let (sender, receiver) = oneshot::channel();
        self.tx
            .send(SupervisorMessage::GetTaskStatus(task_name.into(), sender))
            .map_err(|_| SupervisorHandleError::SendError)?;
        receiver.await.map_err(SupervisorHandleError::RecvError)
    }

    pub async fn get_all_task_statuses(
        &self,
    ) -> Result<HashMap<String, TaskStatus>, SupervisorHandleError> {
        let (sender, receiver) = oneshot::channel();
        self.tx
            .send(SupervisorMessage::GetAllTaskStatuses(sender))
            .map_err(|_| SupervisorHandleError::SendError)?;
        receiver.await.map_err(SupervisorHandleError::RecvError)
    }

    fn is_channel_open(&self) -> bool {
        !self.tx.is_closed()
    }
}

impl std::fmt::Debug for SupervisorHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SupervisorHandle")
            .field("channel_open", &self.is_channel_open())
            .finish()
    }
}
