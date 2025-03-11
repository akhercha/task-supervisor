use futures::future::select_all;
use tokio::task::JoinHandle;

#[derive(Debug)]
pub(crate) struct TaskGroup<T> {
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
        let (_, _, remaining) = select_all(handles).await;
        for task in remaining {
            task.abort();
        }
    }
}
