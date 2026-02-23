use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use task_supervisor::{SupervisedTask, TaskResult};

/// Increments its `run_count` and completes after 100ms.
#[allow(unused)]
#[derive(Clone, Default)]
pub struct CompletingTask {
    pub run_count: Arc<AtomicUsize>,
}

impl SupervisedTask for CompletingTask {
    async fn run(&mut self) -> TaskResult {
        self.run_count.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok(())
    }
}

/// Increments its `run_count` and fails immediately.
#[allow(unused)]
#[derive(Clone)]
pub struct FailingTask {
    pub run_count: Arc<AtomicUsize>,
}

#[allow(clippy::useless_conversion)]
impl SupervisedTask for FailingTask {
    async fn run(&mut self) -> TaskResult {
        self.run_count.fetch_add(1, Ordering::SeqCst);
        Err(anyhow::anyhow!("Task failed!").into())
    }
}

/// Runs forever while `run_flag` is True. Else, completes.
#[allow(unused)]
#[derive(Clone)]
pub struct HealthyTask {
    pub run_flag: Arc<std::sync::atomic::AtomicBool>,
}

impl SupervisedTask for HealthyTask {
    async fn run(&mut self) -> TaskResult {
        while self.run_flag.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Ok(())
    }
}

/// Completes immediately.
#[allow(unused)]
#[derive(Clone)]
pub struct ImmediateCompleteTask;

impl SupervisedTask for ImmediateCompleteTask {
    async fn run(&mut self) -> TaskResult {
        Ok(())
    }
}

/// Fails immediately.
#[allow(unused)]
#[derive(Clone)]
pub struct ImmediateFailTask;

#[allow(clippy::useless_conversion)]
impl SupervisedTask for ImmediateFailTask {
    async fn run(&mut self) -> TaskResult {
        Err(anyhow::anyhow!("Immediate failure!").into())
    }
}
