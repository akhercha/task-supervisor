use std::{collections::HashMap, time::Duration};

use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{supervisor::TaskId, task::TaskHandle, SupervisedTask, Supervisor};

// TODO: We should be able to configure:
// * max_restarts,
// * base_restart_delay,
pub struct SupervisorBuilder<T: SupervisedTask> {
    tasks: HashMap<TaskId, TaskHandle<T>>,
}

impl<T: SupervisedTask> SupervisorBuilder<T> {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
        }
    }

    pub fn with_task(mut self, task: T) -> Self {
        self.tasks.insert(Uuid::new_v4(), TaskHandle::new(task));
        self
    }

    pub fn with_tasks<I>(mut self, tasks: I) -> Self
    where
        I: IntoIterator<Item = T>,
    {
        for task in tasks {
            self = self.with_task(task);
        }
        self
    }

    pub fn build(self) -> Supervisor<T> {
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

impl<T: SupervisedTask> Default for SupervisorBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}
