mod common;

use common::HealthyTask;
use std::sync::Arc;
use task_supervisor::{SupervisorBuilder, TaskStatus};
use tokio::time::{advance, pause};

#[tokio::test]
async fn test_custom_configuration() {
    pause();
    let supervisor = SupervisorBuilder::new()
        .with_timeout_threshold(std::time::Duration::from_millis(100))
        .with_heartbeat_interval(std::time::Duration::from_millis(20))
        .with_health_check_initial_delay(std::time::Duration::from_millis(50))
        .with_health_check_interval(std::time::Duration::from_millis(50))
        .with_max_restart_attempts(2)
        .with_base_restart_delay(std::time::Duration::from_millis(50))
        .build();
    let handle = supervisor.run();

    let task = HealthyTask {
        run_flag: Arc::new(std::sync::atomic::AtomicBool::new(true)),
    };
    handle.add_task("custom_config_task", task).unwrap();

    advance(std::time::Duration::from_millis(300)).await;
    let status = handle
        .get_task_status("custom_config_task")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, TaskStatus::Dead); // Should exceed max restarts faster
}
