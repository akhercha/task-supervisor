use std::{collections::HashMap, time::Instant};

use tokio::sync::mpsc;

use crate::task::{SupervisedTask, TaskHandle};

#[derive(Debug)]
pub struct Heartbeat {
    task_name: String,
    timestamp: Instant,
}

pub struct Supervisor<T: SupervisedTask> {
    tasks: HashMap<String, TaskHandle<T>>,
    rx_heartbeats: mpsc::UnboundedReceiver<Heartbeat>,
    tx_heartbeats: mpsc::UnboundedSender<Heartbeat>,
}

impl<T: SupervisedTask> Supervisor<T> {
    pub async fn run_and_supervise(&mut self) {}
}

pub struct SupervisorBuilder<T: SupervisedTask> {
    tasks: HashMap<String, TaskHandle<T>>,
}

impl<T: SupervisedTask> SupervisorBuilder<T> {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
        }
    }

    pub fn with_task(mut self, task: T) -> Self {
        let task_name = match task.name() {
            Some(name) => name.to_string(),
            None => "caca".to_string(),
        };
        self.tasks.insert(task_name, TaskHandle::new(task));
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
        let (tx_heartbeats, rx_heartbeats) = mpsc::unbounded_channel();
        let supervisor = Supervisor {
            tasks: self.tasks,
            rx_heartbeats,
            tx_heartbeats,
        };
        supervisor
    }
}
