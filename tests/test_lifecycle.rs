mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use task_supervisor::{
    CancellationToken, SupervisedTask, SupervisorBuilder, TaskResult, TaskStatus,
};
use tokio::time::pause;

use common::{runs, sleep_ms, Completes, Cooperative, Failing, Panicking};

fn builder() -> SupervisorBuilder {
    SupervisorBuilder::new()
        .with_max_restart_attempts(3)
        .with_base_restart_delay(Duration::from_millis(100))
        .with_max_restart_delay(Duration::from_millis(100))
}

#[tokio::test]
async fn task_runs_then_completes() {
    pause();
    let handle = builder().spawn();
    let task = Completes {
        after: Duration::from_millis(50),
    };
    handle.add_task("t", task).await.unwrap();

    assert_eq!(handle.task_status("t").await.unwrap(), TaskStatus::Running);
    sleep_ms(100).await;
    assert_eq!(
        handle.task_status("t").await.unwrap(),
        TaskStatus::Completed
    );
}

#[tokio::test]
async fn failing_task_restarts_with_backoff_then_dies() {
    pause();
    let handle = builder().spawn();
    let task = Failing::default();
    handle.add_task("t", task.clone()).await.unwrap();

    // Fails at t=0, restarts at 100, 200, 300ms, then dies.
    sleep_ms(1).await;
    assert_eq!(
        handle.task_status("t").await.unwrap(),
        TaskStatus::Restarting
    );
    assert_eq!(runs(&task.runs), 1);

    sleep_ms(1000).await;
    assert_eq!(handle.task_status("t").await.unwrap(), TaskStatus::Dead);
    assert_eq!(runs(&task.runs), 4, "initial run + 3 restarts");
}

#[tokio::test]
async fn backoff_doubles_and_is_capped() {
    pause();
    let handle = SupervisorBuilder::new()
        .with_max_restart_attempts(4)
        .with_base_restart_delay(Duration::from_millis(100))
        .with_max_restart_delay(Duration::from_millis(250))
        .spawn();
    let task = Failing::default();
    handle.add_task("t", task.clone()).await.unwrap();

    // Delays: 100, 200, 250 (capped), 250 → runs at t=0, 100, 300, 550, 800.
    sleep_ms(50).await;
    assert_eq!(runs(&task.runs), 1);
    sleep_ms(100).await; // t=150
    assert_eq!(runs(&task.runs), 2);
    sleep_ms(200).await; // t=350
    assert_eq!(runs(&task.runs), 3);
    sleep_ms(250).await; // t=600
    assert_eq!(runs(&task.runs), 4);
    sleep_ms(250).await; // t=850
    assert_eq!(runs(&task.runs), 5);
    assert_eq!(handle.task_status("t").await.unwrap(), TaskStatus::Dead);
}

#[tokio::test]
async fn unlimited_restarts_never_die() {
    pause();
    let handle = SupervisorBuilder::new()
        .with_unlimited_restarts()
        .with_base_restart_delay(Duration::from_millis(50))
        .with_max_restart_delay(Duration::from_millis(50))
        .spawn();
    let task = Failing::default();
    handle.add_task("t", task.clone()).await.unwrap();

    sleep_ms(1000).await;
    assert_ne!(handle.task_status("t").await.unwrap(), TaskStatus::Dead);
    assert!(runs(&task.runs) > 10);
}

/// Fails immediately, except on its third run where it stays up for 1s first.
#[derive(Clone, Default)]
struct FlakyThenStable {
    runs: Arc<AtomicUsize>,
}

impl SupervisedTask for FlakyThenStable {
    async fn run(self, _cancel: CancellationToken) -> TaskResult {
        let run = self.runs.fetch_add(1, Ordering::SeqCst) + 1;
        if run == 3 {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        Err("boom".into())
    }
}

#[tokio::test]
async fn long_running_task_resets_restart_budget() {
    pause();
    let handle = SupervisorBuilder::new()
        .with_max_restart_attempts(2)
        .with_base_restart_delay(Duration::from_millis(10))
        .with_max_restart_delay(Duration::from_millis(10))
        .with_stable_after(Duration::from_millis(500))
        .spawn();
    let task = FlakyThenStable::default();
    handle.add_task("t", task.clone()).await.unwrap();

    // Runs 1 and 2 fail fast (budget exhausted). Run 3 lasts 1s > stable_after,
    // so its failure resets the budget: runs 4 and 5 happen before death.
    sleep_ms(2000).await;
    assert_eq!(handle.task_status("t").await.unwrap(), TaskStatus::Dead);
    assert_eq!(runs(&task.runs), 5);
}

#[tokio::test]
async fn panicking_task_is_restarted() {
    pause();
    let handle = SupervisorBuilder::new()
        .with_max_restart_attempts(1)
        .with_base_restart_delay(Duration::from_millis(10))
        .spawn();
    let task = Panicking::default();
    handle.add_task("t", task.clone()).await.unwrap();

    sleep_ms(100).await;
    assert_eq!(handle.task_status("t").await.unwrap(), TaskStatus::Dead);
    assert_eq!(runs(&task.runs), 2);
}

/// Regression: a restart delay that overflows the clock must not panic the supervisor.
#[tokio::test]
async fn huge_restart_delay_keeps_task_in_backoff() {
    pause();
    let handle = SupervisorBuilder::new()
        .with_base_restart_delay(Duration::MAX)
        .with_max_restart_delay(Duration::MAX)
        .with_task("t", Failing::default())
        .spawn();

    sleep_ms(100).await;
    assert_eq!(
        handle.task_status("t").await.unwrap(),
        TaskStatus::Restarting
    );
    assert!(handle.shutdown().await.is_ok());
}

#[tokio::test]
async fn tasks_are_supervised_independently() {
    pause();
    let handle = builder()
        .with_task("steady", Cooperative::default())
        .with_task("flaky", Failing::default())
        .spawn();

    sleep_ms(1000).await;
    let statuses = handle.task_statuses().await.unwrap();
    assert_eq!(statuses["steady"], TaskStatus::Running);
    assert_eq!(statuses["flaky"], TaskStatus::Dead);
    assert_eq!(statuses.len(), 2);
}
