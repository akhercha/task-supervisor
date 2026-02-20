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
    /// Request to add a new running Task
    AddTask(TaskName, DynTask),
    /// Request to restart a Task
    RestartTask(TaskName),
    /// Request to kill a Task
    KillTask(TaskName),
    /// Request the status of a Task
    GetTaskStatus(TaskName, oneshot::Sender<Option<TaskStatus>>),
    /// Request the status of all Tasks
    GetAllTaskStatuses(oneshot::Sender<HashMap<TaskName, TaskStatus>>),
    /// Request sent to shutdown all the running Tasks
    Shutdown,
}

/// Handle used to interact with the `Supervisor`.
///
/// Cloning this handle is cheap. All clones share the same connection to
/// the supervisor. Multiple clones can call `wait()` concurrently and all
/// will receive the result.
#[derive(Clone)]
pub struct SupervisorHandle {
    pub(crate) tx: mpsc::UnboundedSender<SupervisorMessage>,
    /// Receives the supervisor's final result. Using a `watch` channel
    /// allows multiple concurrent `wait()` callers to all see the outcome,
    /// unlike `JoinSet` which would consume the result on the first `join_next()`.
    result_rx: watch::Receiver<Option<Result<(), SupervisorError>>>,
}

impl Drop for SupervisorHandle {
    /// Automatically shuts down the supervisor when the handle is dropped.
    fn drop(&mut self) {
        if self.is_channel_open() {
            let _ = self.shutdown();
        }
    }
}

impl SupervisorHandle {
    /// Creates a new `SupervisorHandle`.
    ///
    /// Spawns a background task that awaits the supervisor's `JoinHandle` and
    /// broadcasts the result via a `watch` channel.
    pub(crate) fn new(
        join_handle: tokio::task::JoinHandle<Result<(), SupervisorError>>,
        tx: mpsc::UnboundedSender<SupervisorMessage>,
    ) -> Self {
        let (result_tx, result_rx) = watch::channel(None);

        tokio::spawn(async move {
            let result = match join_handle.await {
                Ok(supervisor_result) => supervisor_result,
                // Supervisor task was cancelled/panicked — treat as Ok
                Err(_join_error) => Ok(()),
            };
            let _ = result_tx.send(Some(result));
        });

        Self { tx, result_rx }
    }

    /// Waits for the supervisor to complete its execution.
    ///
    /// Multiple callers can `wait()` concurrently; all will receive the same result.
    ///
    /// # Returns
    /// - `Ok(())` if the supervisor completed successfully.
    /// - `Err(SupervisorError)` if the supervisor returned an error.
    pub async fn wait(&self) -> Result<(), SupervisorError> {
        let mut rx = self.result_rx.clone();
        // Wait until the value is Some (i.e., the supervisor has finished).
        loop {
            if let Some(result) = rx.borrow_and_update().clone() {
                return result;
            }
            // Value is still None — wait for a change.
            if rx.changed().await.is_err() {
                // Sender dropped without sending — supervisor is gone.
                return Ok(());
            }
        }
    }

    /// Adds a new task to the supervisor.
    ///
    /// # Arguments
    /// - `task_name`: The unique name of the task.
    /// - `task`: The task to be added, which must implement `SupervisedTask`.
    ///
    /// # Returns
    /// - `Ok(())` if the message was sent successfully.
    /// - `Err(SendError)` if the supervisor is no longer running.
    pub fn add_task<T: SupervisedTask + Clone>(
        &self,
        task_name: &str,
        task: T,
    ) -> Result<(), SupervisorHandleError> {
        self.tx
            .send(SupervisorMessage::AddTask(task_name.into(), Box::new(task)))
            .map_err(|_| SupervisorHandleError::SendError)
    }

    /// Requests the supervisor to restart a specific task.
    ///
    /// # Arguments
    /// - `task_name`: The name of the task to restart.
    ///
    /// # Returns
    /// - `Ok(())` if the message was sent successfully.
    /// - `Err(SendError)` if the supervisor is no longer running.
    pub fn restart(&self, task_name: &str) -> Result<(), SupervisorHandleError> {
        self.tx
            .send(SupervisorMessage::RestartTask(task_name.into()))
            .map_err(|_| SupervisorHandleError::SendError)
    }

    /// Requests the supervisor to kill a specific task.
    ///
    /// # Arguments
    /// - `task_name`: The name of the task to kill.
    ///
    /// # Returns
    /// - `Ok(())` if the message was sent successfully.
    /// - `Err(SendError)` if the supervisor is no longer running.
    pub fn kill_task(&self, task_name: &str) -> Result<(), SupervisorHandleError> {
        self.tx
            .send(SupervisorMessage::KillTask(task_name.into()))
            .map_err(|_| SupervisorHandleError::SendError)
    }

    /// Requests the supervisor to shut down all tasks and stop supervision.
    ///
    /// # Returns
    /// - `Ok(())` if the message was sent successfully.
    /// - `Err(SendError)` if the supervisor is no longer running.
    pub fn shutdown(&self) -> Result<(), SupervisorHandleError> {
        self.tx
            .send(SupervisorMessage::Shutdown)
            .map_err(|_| SupervisorHandleError::SendError)
    }

    /// Queries the status of a specific task asynchronously.
    ///
    /// # Arguments
    /// - `task_name`: The name of the task to query.
    ///
    /// # Returns
    /// - `Ok(Some(TaskStatus))` if the task exists and its status is returned.
    /// - `Ok(None)` if the task does not exist.
    /// - `Err(RecvError)` if communication with the supervisor fails (e.g., it has shut down).
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

    /// Queries the statuses of all tasks asynchronously.
    ///
    /// # Returns
    /// - `Ok(HashMap<TaskName, TaskStatus>)` containing the statuses of all tasks.
    /// - `Err(RecvError)` if communication with the supervisor fails (e.g., it has shut down).
    pub async fn get_all_task_statuses(
        &self,
    ) -> Result<HashMap<String, TaskStatus>, SupervisorHandleError> {
        let (sender, receiver) = oneshot::channel();
        self.tx
            .send(SupervisorMessage::GetAllTaskStatuses(sender))
            .map_err(|_| SupervisorHandleError::SendError)?;
        receiver.await.map_err(SupervisorHandleError::RecvError)
    }

    /// Checks if the supervisor channel is still open.
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
