use tokio::{sync::mpsc, task::JoinHandle};

use crate::{messaging::SupervisorMessage, SupervisedTask};

#[derive(Debug)]
pub struct SupervisorHandle<T: SupervisedTask> {
    pub join_handle: JoinHandle<()>,
    pub tx: mpsc::UnboundedSender<SupervisorMessage<T>>,
}

impl<T: SupervisedTask> SupervisorHandle<T> {
    /// Adds a new task to the running supervisor.
    pub fn add_task(&self, task: T) {
        self.tx.send(SupervisorMessage::AddTask(task)).unwrap();
    }

    /// Waits for the supervisor to complete (i.e., when all tasks are dead).
    pub async fn wait(self) {
        self.join_handle.await.unwrap();
    }
}
