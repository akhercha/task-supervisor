mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use task_supervisor::{SupervisedTask, SupervisorBuilder, TaskResult, TaskStatus};
use tokio::time::pause;

/// A task with both an owned field and a shared field.
/// Used to verify that owned state resets on restart while shared state persists.
#[derive(Clone)]
struct StatefulTask {
    /// Owned field — should be cloned from the original (42) on every restart.
    pub owned_counter: u64,
    /// Shared field — Arc is cloned by reference, so all runs see the same value.
    pub shared_counter: Arc<AtomicUsize>,
    /// Tracks the owned_counter value observed at the start of each run.
    pub observed_owned_values: Arc<std::sync::Mutex<Vec<u64>>>,
}

impl SupervisedTask for StatefulTask {
    async fn run(&mut self) -> TaskResult {
        // Record what owned_counter was when this run started.
        self.observed_owned_values
            .lock()
            .unwrap()
            .push(self.owned_counter);

        // Mutate owned state — this should NOT survive a restart.
        self.owned_counter += 1000;

        // Mutate shared state — this SHOULD survive a restart.
        self.shared_counter.fetch_add(1, Ordering::SeqCst);

        // Fail so we get restarted.
        Err("intentional failure".into())
    }
}

#[tokio::test]
async fn test_owned_fields_reset_on_restart() {
    pause();

    let shared_counter = Arc::new(AtomicUsize::new(0));
    let observed_owned_values = Arc::new(std::sync::Mutex::new(Vec::new()));

    let task = StatefulTask {
        owned_counter: 42,
        shared_counter: shared_counter.clone(),
        observed_owned_values: observed_owned_values.clone(),
    };

    let handle = SupervisorBuilder::new()
        .with_max_restart_attempts(3)
        .with_base_restart_delay(Duration::from_millis(50))
        .with_max_backoff_exponent(0)
        .build()
        .run();

    handle.add_task("stateful", task).unwrap();

    // Wait long enough for initial run + 3 restarts + final death.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let status = handle.get_task_status("stateful").await.unwrap().unwrap();
    assert_eq!(status, TaskStatus::Dead);

    // Shared counter should reflect all 4 runs (initial + 3 restarts).
    assert_eq!(shared_counter.load(Ordering::SeqCst), 4);

    // Every run should have observed owned_counter == 42 (the original value),
    // proving that the +1000 mutation did not carry over to the next restart.
    let observed = observed_owned_values.lock().unwrap();
    assert_eq!(observed.len(), 4);
    for (i, &value) in observed.iter().enumerate() {
        assert_eq!(
            value, 42,
            "Run {i} saw owned_counter={value}, expected 42 (original). \
             Owned state leaked across restarts."
        );
    }
}

#[tokio::test]
async fn test_shared_state_persists_across_restarts() {
    pause();

    let shared_counter = Arc::new(AtomicUsize::new(0));

    let task = StatefulTask {
        owned_counter: 0,
        shared_counter: shared_counter.clone(),
        observed_owned_values: Arc::new(std::sync::Mutex::new(Vec::new())),
    };

    let handle = SupervisorBuilder::new()
        .with_max_restart_attempts(5)
        .with_base_restart_delay(Duration::from_millis(50))
        .with_max_backoff_exponent(0)
        .build()
        .run();

    handle.add_task("shared_state", task).unwrap();

    // Wait for initial + 5 restarts + death.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let status = handle
        .get_task_status("shared_state")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, TaskStatus::Dead);

    // 1 initial run + 5 restarts = 6 total runs.
    assert_eq!(
        shared_counter.load(Ordering::SeqCst),
        6,
        "Shared (Arc) state should accumulate across all runs"
    );
}
