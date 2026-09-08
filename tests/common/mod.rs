#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use task_supervisor::{CancellationToken, SupervisedTask, TaskResult};

/// Runs until cancelled, then exits promptly. Records runs and clean stops.
#[derive(Clone, Default)]
pub struct Cooperative {
    pub runs: Arc<AtomicUsize>,
    pub stopped: Arc<AtomicBool>,
}

impl SupervisedTask for Cooperative {
    async fn run(self, cancel: CancellationToken) -> TaskResult {
        self.runs.fetch_add(1, Ordering::SeqCst);
        cancel.cancelled().await;
        self.stopped.store(true, Ordering::SeqCst);
        Ok(())
    }
}

/// Ignores cancellation and never returns.
#[derive(Clone)]
pub struct Stubborn;

impl SupervisedTask for Stubborn {
    async fn run(self, _cancel: CancellationToken) -> TaskResult {
        std::future::pending().await
    }
}

/// Fails immediately. Records runs.
#[derive(Clone, Default)]
pub struct Failing {
    pub runs: Arc<AtomicUsize>,
}

impl SupervisedTask for Failing {
    async fn run(self, _cancel: CancellationToken) -> TaskResult {
        self.runs.fetch_add(1, Ordering::SeqCst);
        Err("boom".into())
    }
}

/// Completes successfully after `after`.
#[derive(Clone)]
pub struct Completes {
    pub after: Duration,
}

impl SupervisedTask for Completes {
    async fn run(self, _cancel: CancellationToken) -> TaskResult {
        tokio::time::sleep(self.after).await;
        Ok(())
    }
}

/// Panics immediately. Records runs.
#[derive(Clone, Default)]
pub struct Panicking {
    pub runs: Arc<AtomicUsize>,
}

impl SupervisedTask for Panicking {
    async fn run(self, _cancel: CancellationToken) -> TaskResult {
        self.runs.fetch_add(1, Ordering::SeqCst);
        panic!("task panicked");
    }
}

pub fn runs(counter: &Arc<AtomicUsize>) -> usize {
    counter.load(Ordering::SeqCst)
}

pub async fn sleep_ms(ms: u64) {
    tokio::time::sleep(Duration::from_millis(ms)).await;
}
