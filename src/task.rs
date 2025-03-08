use std::time::{Duration, Instant};

use futures::future::select_all;
use tokio::task::JoinHandle;

#[async_trait::async_trait]
pub trait SupervisedTask {
    type Error;

    fn name(&self) -> Option<&str> {
        None
    }

    async fn run_forever(&mut self) -> Result<(), Self::Error>;
}

pub struct TaskHandle<T: SupervisedTask> {
    pub(crate) task: Option<T>,
    pub(crate) handle: Option<tokio::task::JoinHandle<Result<(), T::Error>>>,
    pub(crate) last_heartbeat: Option<Instant>,
    pub(crate) restart_attempts: u32,
    pub(crate) max_restarts: u32,
    pub(crate) base_restart_delay: Duration,
}

impl<T: SupervisedTask> TaskHandle<T> {
    pub fn with_task(mut self, task: T) -> Self {
        self.task = Some(task);
        self
    }
}

impl<T: SupervisedTask> Default for TaskHandle<T> {
    fn default() -> Self {
        Self {
            task: None,
            handle: None,
            last_heartbeat: None,
            restart_attempts: 0,
            max_restarts: 5,
            base_restart_delay: Duration::from_secs(1),
        }
    }
}

impl<T: SupervisedTask> TaskHandle<T> {
    pub fn new(task: T) -> Self {
        Self::default().with_task(task)
    }
}

#[derive(Debug, Default)]
pub struct TaskGroup<T> {
    handles: Vec<JoinHandle<T>>,
}

impl<T> TaskGroup<T> {
    pub const fn new() -> Self {
        Self {
            handles: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_handle(mut self, task: JoinHandle<T>) -> Self {
        self.handles.push(task);
        self
    }

    pub async fn abort_all_if_one_resolves(self) {
        let handles = self.handles;
        if handles.is_empty() {
            return;
        }
        let (_first_res, _, remaining) = select_all(handles).await;
        for task in remaining {
            task.abort();
        }
    }
}
