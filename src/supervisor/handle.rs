use tokio::{
    sync::mpsc,
    task::{JoinError, JoinHandle},
};

use crate::{SupervisedTask, TaskName};

#[derive(Debug, Clone)]
pub enum SupervisorMessage<T: SupervisedTask> {
    /// Sent externally to add a new task to the supervisor
    AddTask(T),
    /// Sent externally to kill task from the supervisor
    KillTask(TaskName),
    /// Sent externally to kill all tasks ran by the supervisor
    Shutdown,
}

/// Handle used to interact with the `Supervisor`.
#[derive(Debug)]
pub struct SupervisorHandle<T: SupervisedTask> {
    pub(crate) join_handle: JoinHandle<()>,
    pub(crate) tx: mpsc::UnboundedSender<SupervisorMessage<T>>,
}

impl<T: SupervisedTask> SupervisorHandle<T> {
    /// Adds a new task to the running supervisor.
    pub fn add_task(&self, task: T) -> Result<(), mpsc::error::SendError<SupervisorMessage<T>>> {
        self.tx.send(SupervisorMessage::AddTask(task))
    }

    /// Waits for the supervisor to complete (i.e., when all tasks are dead).
    pub async fn wait(self) -> Result<(), JoinError> {
        self.join_handle.await
    }

    /// Send a message to kill a task from the Supervisor.
    pub async fn kill_task(
        self,
        task_name: TaskName,
    ) -> Result<(), mpsc::error::SendError<SupervisorMessage<T>>> {
        self.tx.send(SupervisorMessage::KillTask(task_name))
    }

    /// Shutdown all the tasks.
    pub async fn shutdown(self) -> Result<(), mpsc::error::SendError<SupervisorMessage<T>>> {
        self.tx.send(SupervisorMessage::Shutdown)
    }
}
