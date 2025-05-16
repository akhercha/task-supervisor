mod common;

use common::HealthyTask;
use std::{sync::Arc, time::Duration};
use task_supervisor::{SupervisorBuilder, TaskStatus};
use tokio::time::{advance, pause};

#[tokio::test]
async fn test_custom_completion() {
    pause();

    let handle = SupervisorBuilder::new()
        .with_health_check_interval(std::time::Duration::from_millis(50))
        .with_max_restart_attempts(2)
        .with_base_restart_delay(std::time::Duration::from_millis(50))
        .build()
        .run();

    let task = HealthyTask {
        run_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    handle.add_task("custom_config_task", task).unwrap();
    advance(std::time::Duration::from_millis(300)).await;

    let status = handle
        .get_task_status("custom_config_task")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, TaskStatus::Completed);
}

#[tokio::test]
async fn test_custom_infinite() {
    pause();

    let handle = SupervisorBuilder::new()
        .with_health_check_interval(std::time::Duration::from_millis(50))
        .build()
        .run();

    let task = HealthyTask {
        run_flag: Arc::new(std::sync::atomic::AtomicBool::new(true)),
    };
    handle.add_task("custom_config_task", task).unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;

    let status = handle
        .get_task_status("custom_config_task")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, TaskStatus::Healthy);
}
