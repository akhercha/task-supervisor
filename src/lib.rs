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
//! - **[`SupervisorBuilder`]**: Implements the builder pattern to construct a `Supervisor` instance,
//!   allowing tasks to be added before starting supervision.
//!
//! - **[`SupervisorHandle`]**: Provides a handle to interact with a running supervisor, enabling
//!   dynamic addition of new tasks and waiting for all tasks to complete (i.e., reach the `Dead` state).
//!
//! - **[`TaskStatus`] Enum**: Represents the lifecycle states of a supervised task, such as `Created`,
//!   `Healthy`, `Failed`, or `Dead`.
//!
//! ## Usage Example
//!
//! Below is an example demonstrating how to define a supervised task and use the supervisor to manage it:
//!
//! ```rust
//! use anyhow::anyhow;
//! use async_trait::async_trait;
//! use std::time::Duration;
//! use task_supervisor::{SupervisedTask, SupervisorBuilder};
//!
//! // Tasks need to be Cloneable for now for easy restarts
//! #[derive(Clone, Default)]
//! struct MyTask {
//!     pub emoji: char,
//! }
//!
//! impl MyTask {
//!     fn new(emoji: char) -> Self {
//!         Self { emoji }
//!     }
//! }
//!
//! #[async_trait]
//! impl SupervisedTask for MyTask {
//!     type Error = anyhow::Error;
//!
//!     async fn run_forever(&mut self) -> anyhow::Result<()> {
//!         let mut i = 0;
//!         loop {
//!             tokio::time::sleep(Duration::from_secs(1)).await;
//!             println!("{} Task is running!", self.emoji);
//!             i += 1;
//!             if i == 5 {
//!                 println!("{} Task is failing after 5 iterations...", self.emoji);
//!                 return Err(anyhow!("Task failed after 5 iterations"));
//!             }
//!         }
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     // Build the supervisor with initial tasks
//!     let supervisor = SupervisorBuilder::default()
//!         .with_task(MyTask::new('🥴'))
//!         .with_task(MyTask::new('🧑'))
//!         .with_task(MyTask::new('😸'))
//!         .with_task(MyTask::new('👽'))
//!         .build();
//!
//!     // Run the supervisor and get the handle
//!     let handle = supervisor.run();
//!
//!     // Add a new task after 5 seconds
//!     tokio::time::sleep(Duration::from_secs(5)).await;
//!     println!("Adding a new task after 5 seconds...");
//!     handle.add_task(MyTask::new('🆕'));
//!
//!     // Wait for all tasks to die
//!     handle.wait().await;
//!     println!("All tasks died! 🫡");
//! }
//! ```
//!
pub use supervisor::{builder::SupervisorBuilder, handle::SupervisorHandle, Supervisor};
pub use task::{SupervisedTask, TaskStatus};

mod messaging;
mod supervisor;
mod task;
mod task_group;
