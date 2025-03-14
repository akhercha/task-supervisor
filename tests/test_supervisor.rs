mod common;

use std::sync::atomic::Ordering;
use std::sync::Arc;

use tokio::time::{advance, pause};

use task_supervisor::TaskStatus;

use common::{create_supervisor_and_get_handle, FailingTask, HealthyTask, NoHeartbeatTask};

#[tokio::test]
async fn test_healthy_task_remains_healthy() {
    pause();
    let handle = create_supervisor_and_get_handle().await;
    let run_flag = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let task = HealthyTask {
        run_flag: run_flag.clone(),
    };
    handle.add_task("task", task).unwrap();

    advance(std::time::Duration::from_millis(500)).await;
    let status = handle.get_task_status("task").await.unwrap().unwrap();
    assert_eq!(status, TaskStatus::Healthy);

    run_flag.store(false, Ordering::SeqCst);
    advance(std::time::Duration::from_millis(100)).await;
    let status = handle.get_task_status("task").await.unwrap().unwrap();
    assert_eq!(status, TaskStatus::Completed);
}

#[tokio::test]
async fn test_no_heartbeat_task_gets_restarted() {
    pause();
    let handle = create_supervisor_and_get_handle().await;
    let run_flag = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let task = NoHeartbeatTask {
        run_flag: run_flag.clone(),
    };

    handle.add_task("task", task).unwrap();
    advance(std::time::Duration::from_millis(500)).await;

    let status = handle.get_task_status("task").await.unwrap().unwrap();
    assert_eq!(status, TaskStatus::Healthy); // Restarted and running

    run_flag.store(false, Ordering::SeqCst);
    advance(std::time::Duration::from_millis(100)).await;

    let status = handle.get_task_status("task").await.unwrap().unwrap();
    assert_eq!(status, TaskStatus::Completed);
}

#[tokio::test]
async fn test_multiple_tasks() {
    pause();
    let handle = create_supervisor_and_get_handle().await;

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
    let no_heartbeat_task = NoHeartbeatTask {
        run_flag: no_heartbeat_run_flag.clone(),
    };
    handle
        .add_task("no_heartbeat_task", no_heartbeat_task)
        .unwrap();

    advance(std::time::Duration::from_millis(1000)).await;

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
    advance(std::time::Duration::from_millis(100)).await;

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
