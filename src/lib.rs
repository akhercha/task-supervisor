//! # 🤖 task-supervisor
//!
//! [![Crates.io](https://img.shields.io/crates/v/supervisor.svg)](https://crates.io/crates/task-supervisor)
//! [![Docs.rs](https://docs.rs/supervisor/badge.svg)](https://docs.rs/task-supervisor)
//!
//! `task-supervisor` helps you keep Tokio tasks alive.
//! It watches each task, restarts it if it crashes or stops responding, and lets you add, restart, or kill tasks at runtime.
//!
//! ## Install
//!
//! ```bash
//! cargo add task-supervisor
//! ```
//!
//! ## Quick example
//!
//! ```rust,no_run
//! use async_trait::async_trait;
//! use task_supervisor::{SupervisorBuilder, SupervisedTask, TaskResult};
//!
//! #[derive(Clone)]
//! struct Printer;
//!
//! #[async_trait]
//! impl SupervisedTask for Printer {
//!     async fn run(&mut self) -> TaskResult {
//!         println!("hello");
//!         Ok(())
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let supervisor = SupervisorBuilder::default()
//!         .with_task("printer", Printer)
//!         .build()
//!         .run();
//!
//!     supervisor.wait().await?;   // wait until every task finishes or is killed
//!     Ok(())
//! }
//! ```
//!
//! ## What you get
//!
//! * **Automatic restarts** – tasks are relaunched after a crash or hang (exponential back-off).
//! * **Dynamic control** – add, restart, kill, or query tasks through a `SupervisorHandle`.
//! * **Configurable** – tune health-check interval, restart limits, back-off delay, and shutdown policy.
//!
//! ## API overview
//!
//! | TaskHandle method               | Purpose                                                           |
//! | ------------------------------- | ----------------------------------------------------------------- |
//! | `add_task(name, task)`          | Start a new task while running                                    |
//! | `restart(name)`                 | Force a restart                                                   |
//! | `kill_task(name)`               | Stop a task permanently                                           |
//! | `get_task_status(name).await`   | Return `TaskStatus` (`Healthy`, `Failed`, `Completed`, `Dead`, …) |
//! | `get_all_task_statuses().await` | Map of all task states                                            |
//! | `shutdown()`                    | Stop every task and exit                                          |
//!
//! Full documentation lives on docs.rs.
//!
//! ## License
//!
//! [MIT](./LICENSE)

pub use supervisor::{
    builder::SupervisorBuilder,
    handle::{SupervisorHandle, SupervisorHandleError},
    Supervisor, SupervisorError,
};
pub use task::{SupervisedTask, TaskError, TaskResult, TaskStatus};

mod supervisor;
mod task;

pub type TaskName = String;
