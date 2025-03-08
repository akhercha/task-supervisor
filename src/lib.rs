//! # Task Supervisor for Tokio
//!
//! The `task-supervisor` crate provides a smol framework for managing and monitoring asynchronous tasks
//! in Rust using the Tokio runtime. It ensures tasks remain operational by monitoring their health
//! via heartbeats and automatically restarting them if they fail or become unresponsive.
//!
//! ## Key Components
//!
//! - **[`SupervisedTask`] Trait**: Defines the interface for tasks to be supervised. Tasks must
//!   implement the `run_forever` method with their core logic and may optionally provide a `name`.
//!
//! - **[`Supervisor`] Struct**: Manages a collection of tasks, monitors their health using heartbeats,
//!   and restarts them if they crash or exceed a configurable timeout threshold. It uses a message-passing
//!   system to coordinate task supervision.
//!
//! - **[`SupervisorBuilder`]**: Implements the builder to construct a `Supervisor` instance,
//!   allowing tasks to be added.
//!
//! - **[`TaskStatus`] Enum**: Represents the lifecycle states of a supervised task, such as `Created`,
//!   `Healthy`, `Failed`, or `Dead`.
//!
//! ## Usage Example
//!
//! Below is a simple example demonstrating how to define and supervise a task:
//!
//! ```rust
//! use supervisor::{SupervisedTask, SupervisorBuilder};
//! use async_trait::async_trait;
//! use std::time::Duration;
//!
//! #[derive(Clone)]
//! struct MyTask;
//!
//! #[async_trait]
//! impl SupervisedTask for MyTask {
//!     type Error = std::io::Error;
//!
//!     fn name(&self) -> Option<&str> {
//!         Some("my_task")
//!     }
//!
//!     async fn run_forever(&mut self) -> Result<(), Self::Error> {
//!         loop {
//!             tokio::time::sleep(Duration::from_secs(1)).await;
//!             println!("Task is running");
//!         }
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let mut supervisor = SupervisorBuilder::default()
//!         .with_task(MyTask)
//!         .build();
//!
//!     supervisor.run_and_supervise().await;
//! }
//! ```
//!
pub use supervisor::{Supervisor, SupervisorBuilder};
pub use task::{SupervisedTask, TaskStatus};

mod messaging;
mod supervisor;
mod task;
mod task_group;
