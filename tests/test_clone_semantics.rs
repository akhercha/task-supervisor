mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use task_supervisor::{
    CancellationToken, SupervisedTask, SupervisorBuilder, TaskResult, TaskStatus,
};
use tokio::time::pause;

use common::sleep_ms;

/// Owned state must reset on every run; `Arc` state must persist.
#[derive(Clone)]
struct StatefulTask {
    owned_counter: u64,
    shared_counter: Arc<AtomicUsize>,
    runs_seeing_original_owned_value: Arc<AtomicUsize>,
}

impl SupervisedTask for StatefulTask {
    async fn run(mut self, _cancel: CancellationToken) -> TaskResult {
        if self.owned_counter == 42 {
            self.runs_seeing_original_owned_value
                .fetch_add(1, Ordering::SeqCst);
        }
        self.owned_counter += 1000;
        self.shared_counter.fetch_add(1, Ordering::SeqCst);
        Err("intentional failure".into())
    }
}

#[tokio::test]
async fn owned_fields_reset_and_shared_state_persists_across_restarts() {
    pause();
    let task = StatefulTask {
        owned_counter: 42,
        shared_counter: Arc::default(),
        runs_seeing_original_owned_value: Arc::default(),
    };
    let handle = SupervisorBuilder::new()
        .with_restart_limit(3, Duration::from_secs(60))
        .with_base_restart_delay(Duration::from_millis(50))
        .with_task("stateful", task.clone())
        .spawn();

    sleep_ms(1000).await;
    assert_eq!(
        handle.task_status("stateful").await.unwrap(),
        TaskStatus::Dead
    );
    assert_eq!(task.shared_counter.load(Ordering::SeqCst), 4);
    assert_eq!(
        task.runs_seeing_original_owned_value.load(Ordering::SeqCst),
        4
    );
}

#[tokio::test]
async fn anyhow_errors_convert_with_question_mark() {
    fn fallible() -> anyhow::Result<()> {
        anyhow::bail!("anyhow error")
    }

    #[derive(Clone)]
    struct AnyhowTask;

    impl SupervisedTask for AnyhowTask {
        async fn run(self, _cancel: CancellationToken) -> TaskResult {
            fallible()?;
            Ok(())
        }
    }

    pause();
    let handle = SupervisorBuilder::new()
        .with_restart_limit(0, Duration::from_secs(60))
        .with_task("t", AnyhowTask)
        .spawn();

    sleep_ms(10).await;
    assert_eq!(handle.task_status("t").await.unwrap(), TaskStatus::Dead);
}
