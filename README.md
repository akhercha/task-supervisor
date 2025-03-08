# 🤖 task-supervisor

[![Crates.io](https://img.shields.io/crates/v/supervisor.svg)](https://crates.io/crates/task-supervisor)
[![Docs.rs](https://docs.rs/supervisor/badge.svg)](https://docs.rs/supervisor)

The `task-supervisor` crate is a Rust library for managing and monitoring asynchronous tasks within the Tokio runtime. It ensures tasks remain operational by tracking their health via heartbeats and restarting them if they fail or become unresponsive.

It is really smol and simple for now.

## Installation

Add the crate to your `Cargo.toml`:

```toml
[dependencies]
task-supervisor = "0.1.3"  # Replace with the latest version
tokio = { version = "1", features = ["full"] }
async-trait = "0.1"
```

## Usage

### 1. Defining a Supervised Task

Tasks must implement the `SupervisedTask` trait, which requires an error type and the run_forever method:

```rust
use anyhow::bail;
use async_trait::async_trait;
use std::time::Duration;
use task_supervisor::SupervisedTask;

// Tasks need to be Cloneable for now for easy restarts
#[derive(Clone, Default)]
struct MyTask {
    pub emoji: char,
}

impl MyTask {
    fn new(emoji: char) -> Self {
        Self { emoji }
    }
}

#[async_trait]
impl SupervisedTask for MyTask {
    type Error = anyhow::Error;

    async fn run_forever(&mut self) -> anyhow::Result<()> {
        let mut i = 0;
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            println!("{} Task is running!", self.emoji);
            i += 1;
            if i == 5 {
                println!("{} Task is failing after 5 iterations...", self.emoji);
                bail!("Task failed after 5 iterations");
            }
        }
    }
}
```

### 2. Setting Up and Running the Supervisor

Use the `SupervisorBuilder` to create a supervisor and start supervising tasks:

```rust
use task_supervisor::SupervisorBuilder;

#[tokio::main]
async fn main() {
    // Build the supervisor with initial tasks
    let supervisor = SupervisorBuilder::default()
        .with_task(MyTask::new('🥴'))
        .with_task(MyTask::new('🧑'))
        .with_task(MyTask::new('😸'))
        .with_task(MyTask::new('👽'))
        .build();

    // Run the supervisor and get the handle
    let handle = supervisor.run();

    // Add a new task after 5 seconds
    tokio::time::sleep(Duration::from_secs(5)).await;
    println!("Adding a new task after 5 seconds...");
    handle.add_task(MyTask::new('🆕'));

    // Wait for all tasks to die
    handle.wait().await;
    println!("All tasks died! 🫡");
}
```

The supervisor will:
1. Start all initial tasks, each running its run_forever logic.
2. Monitor each task that fail or miss heartbeats & restart them.
3. Allow dynamic addition of new tasks via the SupervisorHandle.
4. Exit when all tasks are marked as Dead (e.g., after exceeding restart attempts).

## Contributing

Contributions are welcomed! Please:
1. Fork the repository on GitHub.
2. Submit a pull request with your changes or open an issue for discussion.

## License
This crate is licensed under the MIT License. See the LICENSE file for details.

