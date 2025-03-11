use std::sync::Arc;

use tokio::{
    sync::{mpsc, Mutex},
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
#[derive(Debug, Clone)]
pub struct SupervisorHandle {
    pub(crate) join_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    pub(crate) tx: mpsc::UnboundedSender<SupervisorMessage>,
}

type SendResult = Result<(), mpsc::error::SendError<SupervisorMessage>>;

impl SupervisorHandle {
    // Constructor for internal use
    pub(crate) fn new(
        join_handle: JoinHandle<()>,
        tx: mpsc::UnboundedSender<SupervisorMessage>,
    ) -> Self {
        Self {
            join_handle: Arc::new(Mutex::new(Some(join_handle))),
            tx,
        }
    }

    /// Waits for the supervisor to complete (i.e., when all tasks completed/are dead).
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the supervisor completed successfully
    /// * `Err(JoinError)` - If the task panicked
    ///
    /// # Panics
    ///
    /// Panics if `wait()` has already been called on any clone of this handle.
    /// Only one caller should await the supervisor's completion.
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

    pub fn add_task<T: SupervisedTask + 'static>(
        &self,
        task_name: TaskName,
        task: T,
    ) -> SendResult {
        self.tx
            .send(SupervisorMessage::AddTask(task_name, Box::new(task)))
    }

    pub fn restart(&self, task_name: TaskName) -> SendResult {
        self.tx.send(SupervisorMessage::RestartTask(task_name))
    }

    pub fn kill_task(&self, task_name: TaskName) -> SendResult {
        self.tx.send(SupervisorMessage::KillTask(task_name))
    }

    pub fn shutdown(&self) -> SendResult {
        self.tx.send(SupervisorMessage::Shutdown)
    }
}
