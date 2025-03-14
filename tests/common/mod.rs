use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use task_supervisor::{SupervisedTask, TaskOutcome};

/// Increments its `run_count` and completes after 100ms.
#[derive(Clone)]
pub struct CompletingTask {
    pub run_count: Arc<AtomicUsize>,
}

#[async_trait]
impl SupervisedTask for CompletingTask {
    async fn run(&mut self) -> Result<TaskOutcome, Box<dyn std::error::Error + Send + Sync>> {
        self.run_count.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok(TaskOutcome::Completed)
    }
}

/// Increments its `run_count` and fails immediatly.
#[derive(Clone)]
pub struct FailingTask {
    pub run_count: Arc<AtomicUsize>,
}

#[async_trait]
impl SupervisedTask for FailingTask {
    async fn run(&mut self) -> Result<TaskOutcome, Box<dyn std::error::Error + Send + Sync>> {
        self.run_count.fetch_add(1, Ordering::SeqCst);
        Ok(TaskOutcome::Failed("Task failed".to_string()))
    }
}

/// Runs forever while `run_flag` is True. Else, completes.
#[derive(Clone)]
pub struct HealthyTask {
    pub run_flag: Arc<std::sync::atomic::AtomicBool>,
}

#[async_trait]
impl SupervisedTask for HealthyTask {
    async fn run(&mut self) -> Result<TaskOutcome, Box<dyn std::error::Error + Send + Sync>> {
        while self.run_flag.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Ok(TaskOutcome::Completed)
    }
}

/// Completes immediatly.
#[derive(Clone)]
pub struct ImmediateCompleteTask;

#[async_trait]
impl SupervisedTask for ImmediateCompleteTask {
    async fn run(&mut self) -> Result<TaskOutcome, Box<dyn std::error::Error + Send + Sync>> {
        Ok(TaskOutcome::Completed)
    }
}

/// Fails immediatly.
#[derive(Clone)]
pub struct ImmediateFailTask;

#[async_trait]
impl SupervisedTask for ImmediateFailTask {
    async fn run(&mut self) -> Result<TaskOutcome, Box<dyn std::error::Error + Send + Sync>> {
        Ok(TaskOutcome::Failed("Immediate failure".to_string()))
    }
}
