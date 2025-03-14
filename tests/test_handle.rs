mod common;

use std::sync::Arc;

use tokio::time::{advance, pause};

use task_supervisor::{SupervisorHandleError, TaskStatus};

use common::{
    create_supervisor_and_get_handle, CompletingTask, HealthyTask, ImmediateCompleteTask,
};

#[tokio::test]
async fn test_add_task_dynamically() {
    pause();
    let handle = create_supervisor_and_get_handle().await;
    let task = CompletingTask {
        run_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    handle.add_task("task", task.clone()).unwrap();

    advance(std::time::Duration::from_millis(150)).await;
    let status = handle.get_task_status("task").await.unwrap().unwrap();
    assert_eq!(status, TaskStatus::Completed);
    assert_eq!(task.run_count.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_restart_task() {
    pause();
    let handle = create_supervisor_and_get_handle().await;
    let task = CompletingTask {
        run_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };

    handle.add_task("task", task.clone()).unwrap();
    advance(std::time::Duration::from_millis(50)).await;

    handle.restart("task").unwrap();
    advance(std::time::Duration::from_secs(3)).await;

    let status = handle.get_task_status("task").await.unwrap().unwrap();
    assert_eq!(status, TaskStatus::Completed);

    // Count should be 2: Initial + restart
    assert_eq!(task.run_count.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_kill_task() {
    pause();
    let handle = create_supervisor_and_get_handle().await;
    let run_flag = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let task = HealthyTask {
        run_flag: run_flag.clone(),
    };
    handle.add_task("task", task).unwrap();

    advance(std::time::Duration::from_millis(50)).await;
    handle.kill_task("task").unwrap();
    advance(std::time::Duration::from_millis(100)).await;

    let status = handle.get_task_status("task").await.unwrap().unwrap();
    assert_eq!(status, TaskStatus::Dead);
}

#[tokio::test]
async fn test_supervisor_shutdown() {
    pause();
    let handle = create_supervisor_and_get_handle().await;
    let run_flag = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let task = HealthyTask {
        run_flag: run_flag.clone(),
    };

    handle.add_task("task", task).unwrap();
    advance(std::time::Duration::from_millis(50)).await;

    handle.shutdown().unwrap();
    advance(std::time::Duration::from_millis(200)).await;

    let result = handle.get_task_status("task").await;
    assert!(matches!(result, Err(SupervisorHandleError::SendError(_))));
}

#[tokio::test]
async fn test_error_handling() {
    pause();
    let handle = create_supervisor_and_get_handle().await;

    handle.shutdown().unwrap();
    advance(std::time::Duration::from_millis(500)).await;

    let result = handle.add_task("task", ImmediateCompleteTask);
    assert!(matches!(result, Err(SupervisorHandleError::SendError(_))));

    let result = handle.get_task_status("non_existent").await;
    assert!(matches!(result, Err(SupervisorHandleError::SendError(_))));
}
