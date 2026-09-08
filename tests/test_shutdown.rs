mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use task_supervisor::SupervisorBuilder;
use tokio::time::{pause, Instant};

use common::{sleep_ms, Cooperative, Stubborn};

#[tokio::test]
async fn shutdown_lets_cooperative_tasks_finish() {
    pause();
    let a = Cooperative::default();
    let b = Cooperative::default();
    let handle = SupervisorBuilder::new()
        .with_task("a", a.clone())
        .with_task("b", b.clone())
        .spawn();

    handle.shutdown().await.unwrap();
    assert!(a.stopped.load(Ordering::SeqCst));
    assert!(b.stopped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn shutdown_is_bounded_by_shutdown_timeout() {
    pause();
    let handle = SupervisorBuilder::new()
        .with_stop_timeout(Duration::from_millis(200))
        .with_task("stubborn", Stubborn)
        .spawn();

    let started = Instant::now();
    handle.shutdown().await.unwrap();
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_millis(200), "{elapsed:?}");
    assert!(elapsed < Duration::from_millis(300), "{elapsed:?}");
}

#[tokio::test]
async fn dropping_last_handle_shuts_down_gracefully() {
    pause();
    let task = Cooperative::default();
    let handle = SupervisorBuilder::new()
        .with_task("t", task.clone())
        .spawn();
    let clone = handle.clone();

    drop(handle);
    sleep_ms(10).await;
    assert!(
        !task.stopped.load(Ordering::SeqCst),
        "one handle still alive"
    );

    drop(clone);
    sleep_ms(10).await;
    assert!(task.stopped.load(Ordering::SeqCst));
}

/// Regression: dropping a clone used to send Shutdown.
#[tokio::test]
async fn dropping_one_clone_keeps_supervisor_running() {
    pause();
    let handle = SupervisorBuilder::new()
        .with_task("t", Cooperative::default())
        .spawn();

    {
        let _short_lived = handle.clone();
    }
    sleep_ms(10).await;
    assert!(handle.task_status("t").await.is_ok());
}

#[tokio::test]
async fn wait_can_be_called_repeatedly_and_concurrently() {
    pause();
    let handle = SupervisorBuilder::new()
        .with_task("t", Cooperative::default())
        .spawn();

    let h = handle.clone();
    tokio::spawn(async move {
        sleep_ms(5).await;
        h.shutdown().await.unwrap();
    });

    let (r1, r2) = tokio::join!(handle.wait(), handle.wait());
    assert!(r1.is_ok() && r2.is_ok());
    assert!(handle.wait().await.is_ok());
}
