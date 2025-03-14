mod common;

use std::sync::Arc;
use std::{sync::atomic::Ordering, time::Duration};

use tokio::time::pause;

use task_supervisor::{SupervisorBuilder, SupervisorHandle, TaskStatus};

use common::{FailingTask, HealthyTask};

fn supervisor_handle() -> SupervisorHandle {
    SupervisorBuilder::new()
        .with_timeout_threshold(Duration::from_millis(200))
        .with_heartbeat_interval(Duration::from_millis(50))
        .with_health_check_initial_delay(Duration::from_millis(100))
        .with_health_check_interval(Duration::from_millis(50))
        .with_max_restart_attempts(3)
        .with_base_restart_delay(Duration::from_millis(100))
        .build()
        .run()
}

#[tokio::test]
async fn test_healthy_task_remains_healthy() {
    pause();
    let handle = supervisor_handle();

    let run_flag = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let task = HealthyTask {
        run_flag: run_flag.clone(),
    };
    handle.add_task("task", task).unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;
    let status = handle.get_task_status("task").await.unwrap().unwrap();
    assert_eq!(status, TaskStatus::Healthy);

    run_flag.store(false, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let status = handle.get_task_status("task").await.unwrap().unwrap();
    assert_eq!(status, TaskStatus::Completed);
}

#[tokio::test]
async fn test_no_heartbeat_task_gets_restarted() {
    pause();
    let handle = supervisor_handle();

    let run_flag = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let task = HealthyTask {
        run_flag: run_flag.clone(),
    };

    handle.add_task("task", task).unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;

    let status = handle.get_task_status("task").await.unwrap().unwrap();
    assert_eq!(status, TaskStatus::Healthy);

    run_flag.store(false, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(150)).await;

    let status = handle.get_task_status("task").await.unwrap().unwrap();
    assert_eq!(status, TaskStatus::Completed);
}

#[tokio::test]
async fn test_multiple_tasks() {
    pause();
    let handle = supervisor_handle();

    let healthy_run_flag = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let healthy_task = HealthyTask {
        run_flag: healthy_run_flag.clone(),
    };
    handle.add_task("healthy_task", healthy_task).unwrap();

    let failing_task = FailingTask {
        run_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    handle
        .add_task("failing_task", failing_task.clone())
        .unwrap();

    let no_heartbeat_run_flag = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let no_heartbeat_task = HealthyTask {
        run_flag: no_heartbeat_run_flag.clone(),
    };
    handle
        .add_task("no_heartbeat_task", no_heartbeat_task)
        .unwrap();

    tokio::time::sleep(Duration::from_millis(1000)).await;

    let healthy_status = handle
        .get_task_status("healthy_task")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(healthy_status, TaskStatus::Healthy);

    let failing_status = handle
        .get_task_status("failing_task")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(failing_status, TaskStatus::Dead);

    let no_heartbeat_status = handle
        .get_task_status("no_heartbeat_task")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(no_heartbeat_status, TaskStatus::Healthy);

    healthy_run_flag.store(false, Ordering::SeqCst);
    no_heartbeat_run_flag.store(false, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(100)).await;

    let healthy_status = handle
        .get_task_status("healthy_task")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(healthy_status, TaskStatus::Completed);

    let no_heartbeat_status = handle
        .get_task_status("no_heartbeat_task")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(no_heartbeat_status, TaskStatus::Completed);
}
