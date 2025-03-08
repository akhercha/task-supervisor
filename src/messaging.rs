use std::time::Instant;

type TaskName = String;

#[derive(Debug, Clone)]
pub(crate) enum SupervisorMessage {
    // Sent by tasks to indicate they are alive
    Heartbeat(Heartbeat),
    // Sent by the supervisor to itself to trigger a task restart
    Restart(TaskName),
}

#[derive(Debug, Clone)]
pub(crate) struct Heartbeat {
    pub(crate) task_name: String,
    pub(crate) timestamp: Instant,
}

impl Heartbeat {
    pub fn new(task_name: String) -> Self {
        Self {
            task_name,
            timestamp: Instant::now(),
        }
    }
}
