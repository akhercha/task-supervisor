//! # Task Supervisor for Tokio
//!
//! The `task-supervisor` crate provides a framework for managing and monitoring asynchronous tasks
//! in Rust using the Tokio runtime. It ensures tasks remain operational by monitoring their health
//! and automatically restarting them if they fail or become unresponsive. The supervisor also supports dynamic
//! task management, allowing tasks to be added, restarted, or killed at runtime.
//!
//! ## Key Features
//!
//! - **Task Supervision**: Monitors tasks using heartbeats and restarts them if they crash or exceed a configurable timeout.
//! - **Dynamic Task Management**: Add, restart, or kill tasks dynamically via the `SupervisorHandle`.
//! - **Task Status Querying**: Retrieve the status of individual tasks or all tasks using the `SupervisorHandle`.
//! - **Configurable Parameters**: Customize timeout thresholds, heartbeat intervals, and health check timings.
//! - **Proper Task Cancellation**: Ensures tasks are properly stopped during restarts or when killed using a `CancellationToken`.
//! - **Task Completion Handling**: Differentiates between tasks that complete successfully and those that fail, with appropriate actions.
//!
//! ## Key Components
//!
//! - **[`SupervisedTask`] Trait**: Defines the interface for tasks to be supervised. Tasks must implement the `run` method,
//!   which contains the task's logic and returns a `Result<TaskOutcome, Box<dyn std::error::Error + Send + Sync>>` indicating
//!   success or failure. Additionally, tasks must implement `clone_task` to allow the supervisor to create new instances for restarts.
//!
//! - **[`Supervisor`] Struct**: Manages a collection of tasks, monitors their health using heartbeats, and restarts them if necessary.
//!   It uses a message-passing system to handle internal events and user commands. The supervisor ensures that tasks are properly
//!   cancelled during restarts or when killed using a `CancellationToken`.
//!
//! - **[`SupervisorBuilder`]**: Provides a builder pattern to construct a `Supervisor` instance with configurable parameters such as
//!   timeout thresholds, heartbeat intervals, and health check timings. Allows adding initial tasks before starting the supervisor.
//!
//! - **[`SupervisorHandle`]**: Offers a handle to interact with a running supervisor, enabling dynamic operations like adding new tasks,
//!   restarting tasks, killing tasks, and shutting down the supervisor. It also provides methods to query the status of individual tasks
//!   or all tasks, and a `wait` method to await the completion of all tasks.
//!
//! - **[`TaskStatus`] Enum**: Represents the lifecycle states of a supervised task, such as `Created`, `Starting`, `Healthy`, `Failed`,
//!   `Completed`, or `Dead`.
//!
//! ## Usage Example
//!
//! Below is an example demonstrating how to define a supervised task, use the supervisor to manage it, and query task statuses:
//!
//! ```rust
//! use async_trait::async_trait;
//! use std::time::Duration;
//! use task_supervisor::{SupervisedTask, SupervisorBuilder, TaskOutcome, SupervisorHandleError};
//!
//! // A task need to be Clonable for now - so we can restart it easily.
//! #[derive(Clone)]
//! struct MyTask {
//!     pub emoji: char,
//! }
//!
//! #[async_trait]
//! impl SupervisedTask for MyTask {
//!     async fn run(&mut self) -> Result<TaskOutcome, Box<dyn std::error::Error + Send + Sync>> {
//!         for _ in 0..15 {
//!             println!("{} Task is running!", self.emoji);
//!             tokio::time::sleep(Duration::from_secs(1)).await;
//!         }
//!         println!("{} Task completed!", self.emoji);
//!         Ok(TaskOutcome::Completed)
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<(), SupervisorHandleError> {
//!     // Build the supervisor with initial tasks
//!     let supervisor = SupervisorBuilder::default().build();
//!
//!     // Run the supervisor and get the handle
//!     let handle = supervisor.run();
//!
//!     let h = handle.clone();
//!     tokio::spawn(async move {
//!         // Add a new task after 5 seconds
//!         tokio::time::sleep(Duration::from_secs(5)).await;
//!         println!("Adding a task after 5 seconds...");
//!         h.add_task("task", MyTask { emoji: '🆕' }).unwrap();
//!
//!         // Query the task status after 2 seconds
//!         tokio::time::sleep(Duration::from_secs(2)).await;
//!         match h.get_task_status("task").await {
//!             Ok(Some(status)) => println!("Task status: {:?}", status),
//!             Ok(None) => println!("Task not found"),
//!             Err(e) => println!("Error getting task status: {}", e),
//!         }
//!
//!         // Restart the task after 5 seconds
//!         tokio::time::sleep(Duration::from_secs(5)).await;
//!         println!("Restarting task after 5 seconds...");
//!         h.restart("task").unwrap();
//!
//!         // Query all task statuses after 2 seconds
//!         tokio::time::sleep(Duration::from_secs(2)).await;
//!         match h.get_all_task_statuses().await {
//!             Ok(statuses) => {
//!                 println!("All task statuses:");
//!                 for (name, status) in statuses {
//!                     println!("  {}: {:?}", name, status);
//!                 }
//!             }
//!             Err(e) => println!("Error getting all task statuses: {}", e),
//!         }
//!
//!         // Kill the task after another 5 seconds
//!         tokio::time::sleep(Duration::from_secs(5)).await;
//!         println!("Killing task after 5 seconds...");
//!         h.kill_task("task").unwrap();
//!
//!         // Shutdown the supervisor
//!         h.shutdown().unwrap();
//!     });
//!
//!     // Wait for all tasks to die
//!     handle.wait().await.unwrap();
//!     println!("All tasks died! 🫡");
//!     Ok(())
//! }
//! ```
//!
pub use supervisor::{
    builder::SupervisorBuilder,
    handle::{SupervisorHandle, SupervisorHandleError},
    Supervisor, SupervisorError,
};
pub use task::{SupervisedTask, TaskOutcome, TaskStatus};

mod supervisor;
mod task;

pub type TaskName = String;
