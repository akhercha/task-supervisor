use std::time::Instant;

use crate::{supervisor::TaskId, SupervisedTask};

#[derive(Debug, Clone)]
pub enum SupervisorMessage<T: SupervisedTask> {
    /// Sent externally to add a new task to the supervisor
    AddTask(T),
}

#[derive(Debug, Clone)]
pub(crate) enum SupervisedTaskMessage {
    /// Sent by tasks to indicate they are alive
    Heartbeat(Heartbeat),
    /// Sent by the supervisor to itself to trigger a task restart
    Restart(TaskId),
}

#[derive(Debug, Clone)]
pub(crate) struct Heartbeat {
    pub(crate) task_id: TaskId,
    pub(crate) timestamp: Instant,
}

impl Heartbeat {
    pub fn new(task_id: TaskId) -> Self {
        Self {
            task_id,
            timestamp: Instant::now(),
        }
    }
}
