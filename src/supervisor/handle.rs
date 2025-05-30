use std::{collections::HashMap, sync::Arc};

use thiserror::Error;
use tokio::{
    sync::{mpsc, oneshot, Mutex},
    task::{JoinError, JoinHandle, JoinSet},
};

use crate::{
    task::{CloneableSupervisedTask, DynTask},
    TaskName, TaskStatus,
};

use super::SupervisorError;

#[derive(Debug, Error)]
pub enum SupervisorHandleError {
    #[error("Failed to send message to supervisor: {0}")]
    SendError(#[from] tokio::sync::mpsc::error::SendError<SupervisorMessage>),
    #[error("Failed to receive response from supervisor: {0}")]
    RecvError(#[from] tokio::sync::oneshot::error::RecvError),
}

pub enum SupervisorMessage {
    /// Request to add a new running Task
    AddTask(TaskName, DynTask),
    /// Request to restart a Task
    RestartTask(TaskName),
    /// Request to kill a Task
    KillTask(TaskName),
    /// Request the status of a  Task
    GetTaskStatus(TaskName, oneshot::Sender<Option<TaskStatus>>),
    /// Request the status of all Task
    GetAllTaskStatuses(oneshot::Sender<HashMap<TaskName, TaskStatus>>),
    /// Request sent to shutdown all the running Tasks
    Shutdown,
}

type SupervisorHandleResult = Result<Result<(), SupervisorError>, JoinError>;

/// Handle used to interact with the `Supervisor`.
#[derive(Debug, Clone)]
pub struct SupervisorHandle {
    pub(crate) tx: mpsc::UnboundedSender<SupervisorMessage>,
    join_set: Arc<Mutex<JoinSet<SupervisorHandleResult>>>,
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
    /// This constructor is for internal use within the crate and initializes the handle
    /// with the provided join handle and message sender.
    ///
    /// # Arguments
    /// - `join_handle`: The `JoinHandle` representing the supervisor's task.
    /// - `tx`: The unbounded sender for sending messages to the supervisor.
    ///
    /// # Returns
    /// A new instance of `SupervisorHandle`.
    pub(crate) fn new(
        join_handle: JoinHandle<Result<(), SupervisorError>>,
        tx: mpsc::UnboundedSender<SupervisorMessage>,
    ) -> Self {
        let mut join_set = JoinSet::new();
        join_set.spawn(join_handle);

        Self {
            tx,
            join_set: Arc::new(Mutex::new(join_set)),
        }
    }

    /// Waits for the supervisor to complete its execution.
    ///
    /// # Returns
    /// - `Ok(())` if the supervisor completed successfully.
    /// - `Err(SupervisorError)` if the supervisor returned an error.
    pub async fn wait(&self) -> Result<(), SupervisorError> {
        let mut join_set = self.join_set.lock().await;

        if join_set.is_empty() {
            return Ok(());
        }

        if let Some(result) = join_set.join_next().await {
            match result {
                Ok(Ok(supervisor_result)) => supervisor_result,
                // TODO: Handle the join_error & propagate it?
                Err(_join_error) => Ok(()),
                _ => Ok(()),
            }
        } else {
            // JoinSet is empty, task already completed
            Ok(())
        }
    }

    /// Adds a new task to the supervisor.
    ///
    /// This method sends a message to the supervisor to add a new task with the specified name.
    ///
    /// # Arguments
    /// - `task_name`: The unique name of the task.
    /// - `task`: The task to be added, which must implement `SupervisedTask`.
    ///
    /// # Returns
    /// - `Ok(())` if the message was sent successfully.
    /// - `Err(SendError)` if the supervisor is no longer running.
    pub fn add_task<T: CloneableSupervisedTask + 'static>(
        &self,
        task_name: &str,
        task: T,
    ) -> Result<(), SupervisorHandleError> {
        self.tx
            .send(SupervisorMessage::AddTask(task_name.into(), Box::new(task)))
            .map_err(SupervisorHandleError::SendError)
    }

    /// Requests the supervisor to restart a specific task.
    ///
    /// This method sends a message to the supervisor to restart the task with the given name.
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
            .map_err(SupervisorHandleError::SendError)
    }

    /// Requests the supervisor to kill a specific task.
    ///
    /// This method sends a message to the supervisor to terminate the task with the given name.
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
            .map_err(SupervisorHandleError::SendError)
    }

    /// Requests the supervisor to shut down all tasks and stop supervision.
    ///
    /// This method sends a message to the supervisor to terminate all tasks and cease operation.
    ///
    /// # Returns
    /// - `Ok(())` if the message was sent successfully.
    /// - `Err(SendError)` if the supervisor is no longer running.
    pub fn shutdown(&self) -> Result<(), SupervisorHandleError> {
        self.tx
            .send(SupervisorMessage::Shutdown)
            .map_err(SupervisorHandleError::SendError)
    }

    /// Queries the status of a specific task asynchronously.
    ///
    /// This method sends a request to the supervisor to retrieve the status of the specified task
    /// and awaits the response.
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
            .map_err(SupervisorHandleError::SendError)?;
        receiver.await.map_err(SupervisorHandleError::RecvError)
    }

    /// Queries the statuses of all tasks asynchronously.
    ///
    /// This method sends a request to the supervisor to retrieve the statuses of all tasks
    /// and awaits the response.
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
            .map_err(SupervisorHandleError::SendError)?;
        receiver.await.map_err(SupervisorHandleError::RecvError)
    }

    /// Checks if the supervisor channel is still open.
    fn is_channel_open(&self) -> bool {
        !self.tx.is_closed()
    }
}
