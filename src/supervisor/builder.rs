use std::{collections::HashMap, time::Duration};

use tokio::sync::mpsc;

use crate::{task::TaskHandle, SupervisedTask, Supervisor, TaskName};

// TODO: We should be able to configure:
// * max_restarts,
// * base_restart_delay,
pub struct SupervisorBuilder {
    tasks: HashMap<TaskName, TaskHandle>,
}

impl SupervisorBuilder {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
        }
    }

    pub fn with_task(mut self, name: String, task: impl SupervisedTask) -> Self {
        self.tasks.insert(name, TaskHandle::new(task));
        self
    }

    pub fn build(self) -> Supervisor {
        let (tx, rx) = mpsc::unbounded_channel();
        let (user_tx, user_rx) = mpsc::unbounded_channel();
        Supervisor {
            tasks: self.tasks,
            timeout_treshold: Duration::from_secs(2),
            tx,
            rx,
            external_tx: user_tx,
            external_rx: user_rx,
        }
    }
}

impl Default for SupervisorBuilder {
    fn default() -> Self {
        Self::new()
    }
}
