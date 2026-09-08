mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use common::{runs, sleep_ms, Cooperative, Failing, Stubborn};
use task_supervisor::{SupervisorBuilder, SupervisorHandleError, TaskStatus};
use tokio::time::{pause, Instant};

fn builder() -> SupervisorBuilder {
    SupervisorBuilder::new()
        .with_max_restart_attempts(5)
        .with_base_restart_delay(Duration::from_millis(100))
        .with_max_restart_delay(Duration::from_millis(100))
        .with_stop_timeout(Duration::from_millis(200))
}

#[tokio::test]
async fn add_duplicate_name_is_rejected() {
    pause();
    let handle = builder().spawn();
    handle.add_task("t", Cooperative::default()).await.unwrap();

    let err = handle
        .add_task("t", Cooperative::default())
        .await
        .unwrap_err();
    assert!(matches!(err, SupervisorHandleError::TaskAlreadyExists(name) if name == "t"));
}

#[tokio::test]
async fn unknown_task_is_reported() {
    pause();
    let handle = builder().spawn();

    assert!(matches!(
        handle.restart_task("nope").await.unwrap_err(),
        SupervisorHandleError::TaskNotFound(name) if name == "nope"
    ));
    assert!(matches!(
        handle.kill_task("nope").await.unwrap_err(),
        SupervisorHandleError::TaskNotFound(_)
    ));
    assert!(matches!(
        handle.task_status("nope").await.unwrap_err(),
        SupervisorHandleError::TaskNotFound(_)
    ));
}

#[tokio::test]
async fn kill_stops_cooperative_task() {
    pause();
    let handle = builder().spawn();
    let task = Cooperative::default();
    handle.add_task("t", task.clone()).await.unwrap();

    handle.kill_task("t").await.unwrap();
    assert_eq!(handle.task_status("t").await.unwrap(), TaskStatus::Dead);
    assert!(task.stopped.load(Ordering::SeqCst));

    // Killing a dead task is a no-op.
    handle.kill_task("t").await.unwrap();
}

#[tokio::test]
async fn kill_of_stubborn_task_resolves_at_stop_timeout() {
    pause();
    let handle = builder().spawn();
    handle.add_task("t", Stubborn).await.unwrap();

    let observer = handle.clone();
    let observed = tokio::spawn(async move {
        sleep_ms(100).await;
        observer.task_status("t").await.unwrap()
    });

    let started = Instant::now();
    handle.kill_task("t").await.unwrap();
    let elapsed = started.elapsed();
    assert!((200..210).contains(&elapsed.as_millis()), "{elapsed:?}");
    assert_eq!(observed.await.unwrap(), TaskStatus::Stopping);
    assert_eq!(handle.task_status("t").await.unwrap(), TaskStatus::Dead);
}

/// Regression: requests during the shutdown drain must fail fast, not wait
/// for `stop_timeout`.
#[tokio::test]
async fn requests_during_shutdown_drain_fail_immediately() {
    pause();
    let handle = SupervisorBuilder::new()
        .with_stop_timeout(Duration::from_secs(5))
        .with_task("t", Stubborn)
        .spawn();

    let shutting_down = handle.clone();
    let shutdown = tokio::spawn(async move { shutting_down.shutdown().await });
    sleep_ms(1).await;

    let started = Instant::now();
    assert!(matches!(
        handle.task_status("t").await,
        Err(SupervisorHandleError::Closed)
    ));
    assert!(started.elapsed() < Duration::from_millis(10));
    assert!(shutdown.await.unwrap().is_ok());
}

/// Regression: a pending backoff restart must not resurrect a killed task.
#[tokio::test]
async fn kill_during_backoff_stays_dead() {
    pause();
    let handle = builder().spawn();
    let task = Failing::default();
    handle.add_task("t", task.clone()).await.unwrap();

    sleep_ms(1).await;
    assert_eq!(
        handle.task_status("t").await.unwrap(),
        TaskStatus::Restarting
    );
    handle.kill_task("t").await.unwrap();
    assert_eq!(handle.task_status("t").await.unwrap(), TaskStatus::Dead);

    sleep_ms(500).await;
    assert_eq!(handle.task_status("t").await.unwrap(), TaskStatus::Dead);
    assert_eq!(runs(&task.runs), 1);
}

/// Regression: a manual restart during backoff must not leave a stale timer
/// that fires a second, unrequested restart.
#[tokio::test]
async fn restart_during_backoff_does_not_double_run() {
    pause();
    let handle = SupervisorBuilder::new()
        .with_max_restart_attempts(5)
        .with_base_restart_delay(Duration::from_secs(1))
        .with_max_restart_delay(Duration::from_secs(1))
        .spawn();
    let task = Failing::default();
    handle.add_task("t", task.clone()).await.unwrap();

    // t=0: run 1 fails, backoff timer at t=1000.
    sleep_ms(100).await;
    handle.restart_task("t").await.unwrap();
    // t=100: run 2 fails, backoff timer at t=1100.
    sleep_ms(1).await;
    assert_eq!(runs(&task.runs), 2);

    sleep_ms(950).await; // t=1051: stale timer from run 1 fired, must be ignored.
    assert_eq!(runs(&task.runs), 2);
    sleep_ms(100).await; // t=1151: run 3.
    assert_eq!(runs(&task.runs), 3);
}

#[tokio::test]
async fn manual_restart_resets_restart_budget() {
    pause();
    let handle = SupervisorBuilder::new()
        .with_max_restart_attempts(1)
        .with_base_restart_delay(Duration::from_millis(100))
        .spawn();
    let task = Failing::default();
    handle.add_task("t", task.clone()).await.unwrap();

    sleep_ms(500).await;
    assert_eq!(handle.task_status("t").await.unwrap(), TaskStatus::Dead);
    assert_eq!(runs(&task.runs), 2);

    handle.restart_task("t").await.unwrap();
    sleep_ms(500).await;
    assert_eq!(handle.task_status("t").await.unwrap(), TaskStatus::Dead);
    assert_eq!(runs(&task.runs), 4, "fresh budget: one run + one restart");
}

#[tokio::test]
async fn restart_running_task_stops_it_then_starts_a_fresh_run() {
    pause();
    let handle = builder().spawn();
    let task = Cooperative::default();
    handle.add_task("t", task.clone()).await.unwrap();

    handle.restart_task("t").await.unwrap();
    assert_eq!(runs(&task.runs), 2);
    assert!(task.stopped.load(Ordering::SeqCst));
    assert_eq!(handle.task_status("t").await.unwrap(), TaskStatus::Running);
}

#[tokio::test]
async fn operations_after_shutdown_report_closed() {
    pause();
    let handle = builder().spawn();
    handle.add_task("t", Cooperative::default()).await.unwrap();
    handle.shutdown().await.unwrap();

    assert!(matches!(
        handle.add_task("u", Cooperative::default()).await,
        Err(SupervisorHandleError::Closed)
    ));
    assert!(matches!(
        handle.task_status("t").await,
        Err(SupervisorHandleError::Closed)
    ));
}
