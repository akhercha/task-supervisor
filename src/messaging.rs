use std::time::Instant;

use crate::supervisor::TaskId;

#[derive(Debug, Clone)]
pub(crate) enum SupervisorMessage {
    // Sent by tasks to indicate they are alive
    Heartbeat(Heartbeat),
    // Sent by the supervisor to itself to trigger a task restart
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
