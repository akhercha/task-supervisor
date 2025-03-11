use tokio::{
    sync::mpsc,
    task::{JoinError, JoinHandle},
};

use crate::{task::DynTask, SupervisedTask, TaskName};

pub enum SupervisorMessage {
    AddTask(TaskName, DynTask),
    RestartTask(TaskName),
    KillTask(TaskName),
    Shutdown,
}

/// Handle used to interact with the `Supervisor`.
#[derive(Debug)]
pub struct SupervisorHandle {
    pub(crate) join_handle: JoinHandle<()>,
    pub(crate) tx: mpsc::UnboundedSender<SupervisorMessage>,
}

type SendResult = Result<(), mpsc::error::SendError<SupervisorMessage>>;

impl SupervisorHandle {
    /// Waits for the supervisor to complete (i.e., when all tasks completed/ are dead).
    pub async fn wait(self) -> Result<(), JoinError> {
        self.join_handle.await
    }

    /// Adds a new task to the running supervisor.
    pub fn add_task<T: SupervisedTask + 'static>(
        &self,
        task_name: TaskName,
        task: T,
    ) -> SendResult {
        self.tx
            .send(SupervisorMessage::AddTask(task_name, Box::new(task)))
    }

    /// Restart a running task.
    pub async fn restart(self, task_name: TaskName) -> SendResult {
        self.tx.send(SupervisorMessage::RestartTask(task_name))
    }

    /// Send a message to kill a task from the Supervisor.
    pub async fn kill_task(self, task_name: TaskName) -> SendResult {
        self.tx.send(SupervisorMessage::KillTask(task_name))
    }

    /// Shutdown all the tasks.
    pub async fn shutdown(self) -> SendResult {
        self.tx.send(SupervisorMessage::Shutdown)
    }
}
