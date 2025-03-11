# 🤖 task-supervisor

[![Crates.io](https://img.shields.io/crates/v/supervisor.svg)](https://crates.io/crates/task-supervisor)
[![Docs.rs](https://docs.rs/supervisor/badge.svg)](https://docs.rs/task-supervisor)

The `task-supervisor` crate is a Rust library for managing and monitoring asynchronous tasks within the Tokio runtime. It ensures tasks remain operational by tracking their health via heartbeats and restarting them if they fail or become unresponsive.

It is really smol and simple for now.

## Installation

Add the crate to your `Cargo.toml`:

```toml
[dependencies]
task-supervisor = "0.1.4"  # Replace with the latest version
tokio = { version = "1", features = ["full"] }
async-trait = "0.1"
```

## Usage

### 1. Defining a Supervised Task

Tasks must implement the `SupervisedTask` trait, which requires implementing the `run` method and the `clone_task` method. The run method defines the task's logic and returns a `TaskOutcome` to indicate completion or failure, while `clone_task` enables task restarting:

```rust
use async_trait::async_trait;
use std::time::Duration;
use task_supervisor::{SupervisedTask, TaskOutcome};

#[derive(Clone)]
struct MyTask {
    pub emoji: char,
}

#[async_trait]
impl SupervisedTask for MyTask {
    async fn run(&mut self) -> Result<TaskOutcome, Box<dyn std::error::Error + Send + Sync>> {
        for _ in 0..15 {
            println!("{} Task is running!", self.emoji);
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        println!("{} Task completed!", self.emoji);
        Ok(TaskOutcome::Completed)
    }

    fn clone_task(&self) -> Box<dyn SupervisedTask> {
        Box::new(self.clone())
    }
}
```

A task can run forever and never return any token.

> [!WARNING]  
> A task must implement `Clone` for now, since we need to be able to clone it for restarts.
> This will probably be updated one day.

### 2. Setting Up and Running the Supervisor

Use the `SupervisorBuilder` to create a supervisor and start supervising tasks. The `SupervisorHandle` allows dynamic task management:

```rust
use task_supervisor::SupervisorBuilder;
use std::time::Duration;

#[tokio::main]
async fn main() {
    // Build the supervisor with initial tasks
    let supervisor = SupervisorBuilder::default().build();

    // Run the supervisor and get the handle
    let handle = supervisor.run();

    let h = handle.clone();
    tokio::spawn(async move {
        // Add a new task after 5 seconds
        tokio::time::sleep(Duration::from_secs(5)).await;
        println!("Adding a task after 5 seconds...");
        h.add_task("task".into(), MyTask { emoji: '🆕' }).unwrap();

        // Restart the task after 5 seconds
        tokio::time::sleep(Duration::from_secs(5)).await;
        println!("Restarting task after 5 seconds...");
        h.restart("task".into()).unwrap();

        // Kill the task after another 5 seconds
        tokio::time::sleep(Duration::from_secs(5)).await;
        println!("Killing task after 5 seconds...");
        h.kill_task("task".into()).unwrap();
    });

    // Wait for all tasks to die
    let _ = handle.wait().await;
    println!("All tasks died! 🫡");
}
```

The supervisor will:
1. Start all initial tasks, each executing its run logic.
2. Monitor tasks via heartbeats, restarting them if they fail or become unresponsive.
3. Enable dynamic addition, restarting, and killing of tasks using the SupervisorHandle.
4. Exit when all tasks are marked as Dead or Completed.

## Contributing

Contributions are welcomed! Please:
1. Fork the repository on GitHub.
2. Submit a pull request with your changes or open an issue for discussion.

## License
This crate is licensed under the MIT License. See the LICENSE file for details.

