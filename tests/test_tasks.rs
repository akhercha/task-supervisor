mod common;

use std::{sync::Arc, time::Duration};
use task_supervisor::{SupervisorBuilder, TaskStatus};
use tokio::time::pause;

use common::{
    CompletingTask, FailingTask, ImmediateCompleteTask, ImmediateDieForeverTask, ImmediateFailTask,
};

#[tokio::test]
async fn test_task_completes_successfully() {
    pause();
    let handle = SupervisorBuilder::new().build().run();
    let task = CompletingTask {
        run_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    handle.add_task("completing_task", task.clone()).unwrap();

    tokio::time::sleep(Duration::from_millis(150)).await;
    let status = handle
        .get_task_status("completing_task")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, TaskStatus::Completed);
    assert_eq!(task.run_count.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_task_fails_and_restarts() {
    pause();
    let handle = SupervisorBuilder::new()
        .with_max_restart_attempts(3)
        .with_health_check_interval(Duration::from_millis(300))
        .with_base_restart_delay(std::time::Duration::from_millis(100))
        .build()
        .run();
    let task = FailingTask {
        run_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    handle.add_task("failing_task", task.clone()).unwrap();

    tokio::time::sleep(Duration::from_millis(1500)).await;
    let status = handle
        .get_task_status("failing_task")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, TaskStatus::Dead);
    assert_eq!(task.run_count.load(std::sync::atomic::Ordering::SeqCst), 4); // Initial + 3 restarts
}

#[tokio::test]
async fn test_immediate_complete_task() {
    pause();
    let handle = SupervisorBuilder::new().build().run();
    let task = ImmediateCompleteTask;
    handle.add_task("immediate_complete", task).unwrap();

    tokio::time::sleep(Duration::from_millis(10)).await;
    let status = handle
        .get_task_status("immediate_complete")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, TaskStatus::Completed);
}

#[tokio::test]
async fn test_immediate_fail_task() {
    pause();
    let handle = SupervisorBuilder::new()
        .with_max_restart_attempts(3)
        .with_base_restart_delay(std::time::Duration::from_millis(100))
        .build()
        .run();
    let task = ImmediateFailTask;
    handle.add_task("immediate_fail", task).unwrap();

    tokio::time::sleep(Duration::from_millis(1500)).await;
    let status = handle
        .get_task_status("immediate_fail")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, TaskStatus::Dead);
}

#[tokio::test]
async fn test_immediate_dead_forever_task() {
    pause();
    let handle = SupervisorBuilder::new()
        .with_max_restart_attempts(100)
        .with_health_check_interval(Duration::from_millis(10))
        .with_base_restart_delay(std::time::Duration::from_millis(100))
        .build()
        .run();
    let task = ImmediateDieForeverTask;
    handle.add_task("immediate_die_forever", task).unwrap();

    tokio::time::sleep(Duration::from_millis(10)).await;

    let status = handle
        .get_task_status("immediate_die_forever")
        .await
        .unwrap()
        .unwrap();

    assert_eq!(status, TaskStatus::Dead);
}
