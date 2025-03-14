use std::{collections::HashMap, sync::Arc};

use thiserror::Error;
use tokio::{
    sync::{mpsc, oneshot, Mutex},
    task::{JoinError, JoinHandle},
};

use crate::{
    task::{CloneableSupervisedTask, DynTask},
    TaskName, TaskStatus,
};

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

/// Handle used to interact with the `Supervisor`.
#[derive(Debug, Clone)]
pub struct SupervisorHandle {
    pub(crate) join_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    pub(crate) tx: mpsc::UnboundedSender<SupervisorMessage>,
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
        join_handle: JoinHandle<()>,
        tx: mpsc::UnboundedSender<SupervisorMessage>,
    ) -> Self {
        Self {
            join_handle: Arc::new(Mutex::new(Some(join_handle))),
            tx,
        }
    }

    /// Waits for the supervisor to complete its execution.
    ///
    /// This method consumes the handle and waits for the supervisor task to finish.
    /// It should be called only once per handle, as it takes ownership of the join handle.
    ///
    /// # Returns
    /// - `Ok(())` if the supervisor completed successfully.
    /// - `Err(JoinError)` if the supervisor task panicked.
    ///
    /// # Panics
    /// Panics if `wait()` has already been called on any clone of this handle.
    pub async fn wait(self) -> Result<(), JoinError> {
        let handle_opt = {
            let mut guard = self.join_handle.lock().await;
            guard.take()
        };

        match handle_opt {
            Some(handle) => handle.await,
            None => panic!("SupervisorHandle::wait() was already called on a clone of this handle"),
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
}
