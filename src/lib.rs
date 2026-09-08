//! # task-supervisor
//!
//! Keeps long-lived Tokio tasks alive: restarts them with exponential backoff
//! when they fail or panic, stops them gracefully, and lets you add, restart,
//! kill or inspect tasks at runtime.
//!
//! ```rust,no_run
//! use std::time::Duration;
//! use task_supervisor::{CancellationToken, SupervisedTask, SupervisorBuilder, TaskResult};
//!
//! #[derive(Clone)]
//! struct Heartbeat;
//!
//! impl SupervisedTask for Heartbeat {
//!     async fn run(self, cancel: CancellationToken) -> TaskResult {
//!         loop {
//!             tokio::select! {
//!                 _ = cancel.cancelled() => return Ok(()),
//!                 _ = tokio::time::sleep(Duration::from_secs(1)) => println!("beat"),
//!             }
//!         }
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let handle = SupervisorBuilder::new()
//!         .with_task("heartbeat", Heartbeat)
//!         .spawn();
//!
//!     tokio::signal::ctrl_c().await.unwrap();
//!     handle.shutdown().await.unwrap();
//! }
//! ```
//!
//! ## Task lifecycle
//!
//! ```text
//!            add/with_task
//!                 │
//!                 ▼
//!   ┌────────► Running ──── Ok(()) ────► Completed
//!   │             │
//!   │        Err / panic
//!   │             │
//!   │             ▼
//!   │        Restarting ── budget exhausted ──► Dead
//!   │             │
//!   └── backoff ──┘
//!
//!   kill / restart / shutdown on a Running task → Stopping → Dead (or a new Running)
//! ```
//!
//! * A failed run is restarted after `base_restart_delay * 2^n`, capped at
//!   `max_restart_delay`, for up to `max_restart_attempts` restarts. A run that
//!   lasted at least `stable_after` resets that budget when it fails.
//! * Stopping a task cancels its [`CancellationToken`]; the run then has
//!   `stop_timeout` to return before its future is dropped.
//! * `run` receives a fresh clone of the registered task every time; see
//!   [`SupervisedTask`] for what survives a restart.
//! * Dropping the last [`SupervisorHandle`] shuts the supervisor down gracefully.
//!
//! See [`SupervisorBuilder`] for settings and defaults, [`SupervisorHandle`]
//! for runtime control.

#![warn(missing_docs)]

mod builder;
mod handle;
mod supervisor;
mod task;

pub use builder::SupervisorBuilder;
pub use handle::{SupervisorHandle, SupervisorHandleError};
pub use supervisor::SupervisorError;
pub use task::{SupervisedTask, TaskError, TaskResult, TaskStatus};
pub use tokio_util::sync::CancellationToken;
