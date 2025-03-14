use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use task_supervisor::{SupervisedTask, SupervisorBuilder, TaskOutcome};

// Helper function to create a supervisor with default settings
#[allow(unused)]
pub async fn create_supervisor_and_get_handle() -> task_supervisor::SupervisorHandle {
    let supervisor = SupervisorBuilder::new()
        .with_timeout_threshold(Duration::from_millis(200))
        .with_heartbeat_interval(Duration::from_millis(50))
        .with_health_check_initial_delay(Duration::from_millis(100))
        .with_health_check_interval(Duration::from_millis(10))
        .with_max_restart_attempts(3)
        .with_base_restart_delay(Duration::from_millis(10))
        .build();
    supervisor.run()
}

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

#[derive(Clone)]
pub struct NoHeartbeatTask {
    pub run_flag: Arc<std::sync::atomic::AtomicBool>,
}

#[async_trait]
impl SupervisedTask for NoHeartbeatTask {
    async fn run(&mut self) -> Result<TaskOutcome, Box<dyn std::error::Error + Send + Sync>> {
        while self.run_flag.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Ok(TaskOutcome::Completed)
    }
}

#[derive(Clone)]
pub struct ImmediateCompleteTask;

#[async_trait]
impl SupervisedTask for ImmediateCompleteTask {
    async fn run(&mut self) -> Result<TaskOutcome, Box<dyn std::error::Error + Send + Sync>> {
        Ok(TaskOutcome::Completed)
    }
}

#[derive(Clone)]
pub struct ImmediateFailTask;

#[async_trait]
impl SupervisedTask for ImmediateFailTask {
    async fn run(&mut self) -> Result<TaskOutcome, Box<dyn std::error::Error + Send + Sync>> {
        Ok(TaskOutcome::Failed("Immediate failure".to_string()))
    }
}
