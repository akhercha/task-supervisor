mod common;

use common::HealthyTask;
use std::{sync::Arc, time::Duration};
use task_supervisor::SupervisorBuilder;
use tokio::time::pause;

#[tokio::test]
async fn test_consecutive_waits() {
    pause();

    let handle = SupervisorBuilder::new().build().run();

    let run_flag = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let task = HealthyTask {
        run_flag: run_flag.clone(),
    };
    handle.add_task("task", task).unwrap();

    let handle_clone = handle.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(5)).await;
        handle_clone.shutdown().unwrap();
    });

    let result1 = handle.wait().await;
    let result2 = handle.wait().await;
    let result3 = handle.wait().await;

    assert!(result1.is_ok());
    assert!(result2.is_ok());
    assert!(result3.is_ok());
}

#[tokio::test]
async fn test_parallel_waits() {
    pause();

    let handle = SupervisorBuilder::new().build().run();

    let run_flag = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let task = HealthyTask {
        run_flag: run_flag.clone(),
    };
    handle.add_task("task", task).unwrap();

    let handle_clone = handle.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(5)).await;
        handle_clone.shutdown().unwrap();
    });

    let handle1 = handle.clone();
    let handle2 = handle.clone();
    let handle3 = handle.clone();

    let (result1, result2, result3) = tokio::join!(handle1.wait(), handle2.wait(), handle3.wait());

    assert!(result1.is_ok());
    assert!(result2.is_ok());
    assert!(result3.is_ok());
}
