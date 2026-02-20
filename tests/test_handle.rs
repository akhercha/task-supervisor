mod common;

use std::{sync::Arc, time::Duration};

use tokio::time::{advance, pause};

use task_supervisor::{SupervisorBuilder, SupervisorHandle, SupervisorHandleError, TaskStatus};

use common::{CompletingTask, HealthyTask, ImmediateCompleteTask};

fn supervisor_handle() -> SupervisorHandle {
    SupervisorBuilder::new()
        .with_health_check_interval(Duration::from_millis(100))
        .with_max_restart_attempts(3)
        .with_base_restart_delay(Duration::from_millis(100))
        .build()
        .run()
}

#[tokio::test]
async fn test_add_task_dynamically() {
    pause();

    let handle = supervisor_handle();

    let task = CompletingTask {
        run_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    handle.add_task("task", task.clone()).unwrap();

    tokio::time::sleep(Duration::from_millis(150)).await;
    let status = handle.get_task_status("task").await.unwrap().unwrap();
    assert_eq!(status, TaskStatus::Completed);
    assert_eq!(task.run_count.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_restart_task() {
    pause();

    let handle = supervisor_handle();

    let task = CompletingTask {
        run_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };

    handle.add_task("task", task.clone()).unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    handle.restart("task").unwrap();
    tokio::time::sleep(Duration::from_secs(3)).await;

    let status = handle.get_task_status("task").await.unwrap().unwrap();
    assert_eq!(status, TaskStatus::Completed);

    // Count should be 2: Initial + restart
    assert_eq!(task.run_count.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_kill_task() {
    pause();

    let handle = supervisor_handle();

    let run_flag = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let task = HealthyTask {
        run_flag: run_flag.clone(),
    };
    handle.add_task("task", task).unwrap();

    advance(Duration::from_millis(50)).await;
    handle.kill_task("task").unwrap();
    advance(Duration::from_millis(100)).await;

    let status = handle.get_task_status("task").await.unwrap().unwrap();
    assert_eq!(status, TaskStatus::Dead);
}

#[tokio::test]
async fn test_supervisor_shutdown() {
    pause();

    let handle = supervisor_handle();

    let run_flag = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let task = HealthyTask {
        run_flag: run_flag.clone(),
    };

    handle.add_task("task", task).unwrap();
    advance(Duration::from_millis(50)).await;

    handle.shutdown().unwrap();
    advance(Duration::from_millis(200)).await;

    let result = handle.get_task_status("task").await;
    assert!(matches!(result, Err(SupervisorHandleError::SendError)));
}

#[tokio::test]
async fn test_error_handling() {
    pause();

    let handle = supervisor_handle();

    handle.shutdown().unwrap();
    advance(Duration::from_millis(500)).await;

    let result = handle.add_task("task", ImmediateCompleteTask);
    assert!(matches!(result, Err(SupervisorHandleError::SendError)));

    let result = handle.get_task_status("non_existent").await;
    assert!(matches!(result, Err(SupervisorHandleError::SendError)));
}
