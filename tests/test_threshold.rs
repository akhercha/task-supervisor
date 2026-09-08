mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use task_supervisor::{SupervisorBuilder, SupervisorError, TaskStatus};
use tokio::time::pause;

use common::{sleep_ms, Cooperative, Failing};

fn builder() -> SupervisorBuilder {
    SupervisorBuilder::new()
        .with_max_restart_attempts(0)
        .with_base_restart_delay(Duration::from_millis(10))
}

#[tokio::test]
async fn supervisor_shuts_down_when_threshold_reached() {
    pause();
    let steady = Cooperative::default();
    let handle = builder()
        .with_dead_tasks_threshold(0.5)
        .with_task("steady", steady.clone())
        .with_task("flaky", Failing::default())
        .spawn();

    let err = handle.wait().await.unwrap_err();
    assert!(
        matches!(
            err,
            SupervisorError::TooManyDeadTasks {
                dead: 1,
                total: 2,
                ..
            }
        ),
        "{err}"
    );
    assert_eq!(
        err.to_string(),
        "too many dead tasks: 1/2 (50%) >= threshold 50%"
    );
    assert!(
        steady.stopped.load(Ordering::SeqCst),
        "surviving tasks are stopped gracefully"
    );
}

#[tokio::test]
async fn supervisor_keeps_running_below_threshold() {
    pause();
    let handle = builder()
        .with_dead_tasks_threshold(0.6)
        .with_task("steady", Cooperative::default())
        .with_task("flaky", Failing::default())
        .spawn();

    sleep_ms(100).await;
    assert_eq!(
        handle.task_status("steady").await.unwrap(),
        TaskStatus::Running
    );
    assert_eq!(handle.task_status("flaky").await.unwrap(), TaskStatus::Dead);
    assert!(handle.shutdown().await.is_ok());
}

/// Regression: `0.0` means "any dead task", not "shut down at the first event".
#[tokio::test]
async fn threshold_zero_waits_for_a_death() {
    pause();
    let handle = builder()
        .with_dead_tasks_threshold(0.0)
        .with_task("steady", Cooperative::default())
        .spawn();

    sleep_ms(100).await;
    assert_eq!(
        handle.task_status("steady").await.unwrap(),
        TaskStatus::Running
    );

    handle.kill_task("steady").await.unwrap();
    assert!(matches!(
        handle.wait().await,
        Err(SupervisorError::TooManyDeadTasks {
            dead: 1,
            total: 1,
            ..
        })
    ));
}
