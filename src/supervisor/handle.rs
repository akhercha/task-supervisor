use tokio::{
    sync::mpsc,
    task::{JoinError, JoinHandle},
};

use crate::{SupervisedTask, TaskName};

type SendResult<T> = Result<(), mpsc::error::SendError<SupervisorMessage<T>>>;

#[derive(Debug, Clone)]
pub enum SupervisorMessage<T: SupervisedTask> {
    AddTask(T),
    RestartTask(TaskName),
    KillTask(TaskName),
    Shutdown,
}

/// Handle used to interact with the `Supervisor`.
#[derive(Debug)]
pub struct SupervisorHandle<T: SupervisedTask> {
    pub(crate) join_handle: JoinHandle<()>,
    pub(crate) tx: mpsc::UnboundedSender<SupervisorMessage<T>>,
}

impl<T: SupervisedTask> SupervisorHandle<T> {
    /// Waits for the supervisor to complete (i.e., when all tasks completed/ are dead).
    pub async fn wait(self) -> Result<(), JoinError> {
        self.join_handle.await
    }

    /// Adds a new task to the running supervisor.
    pub fn add_task(&self, task: T) -> SendResult<T> {
        self.tx.send(SupervisorMessage::AddTask(task))
    }

    /// Restart a running task.
    pub async fn restart(self, task_name: TaskName) -> SendResult<T> {
        self.tx.send(SupervisorMessage::RestartTask(task_name))
    }

    /// Send a message to kill a task from the Supervisor.
    pub async fn kill_task(self, task_name: TaskName) -> SendResult<T> {
        self.tx.send(SupervisorMessage::KillTask(task_name))
    }

    /// Shutdown all the tasks.
    pub async fn shutdown(self) -> SendResult<T> {
        self.tx.send(SupervisorMessage::Shutdown)
    }
}
