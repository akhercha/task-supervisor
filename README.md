# 🤖 task-supervisor

[![Crates.io](https://img.shields.io/crates/v/supervisor.svg)](https://crates.io/crates/supervisor)
[![Docs.rs](https://docs.rs/supervisor/badge.svg)](https://docs.rs/supervisor)

The `task-supervisor` crate is a Rust library for managing and monitoring asynchronous tasks within the Tokio runtime. It ensures tasks remain operational by tracking their health via heartbeats and restarting them if they fail or become unresponsive.

It is really smol and simple for now.

## Installation

Add the crate to your `Cargo.toml`:

```toml
[dependencies]
task-supervisor = "0.1.2"  # Replace with the latest version
tokio = { version = "1", features = ["full"] }
async-trait = "0.1"
```

## Usage

### 1. Defining a Supervised Task

Tasks must implement the `SupervisedTask` trait, which requires an error type and the run_forever method:

```rust
use std::time::Duration;

use task_supervisor::SupervisedTask;

// Tasks needs to be Clonable for now for easy restarts - will probably change.
#[derive(Clone)]
struct MyTask;

#[async_trait::async_trait]
impl SupervisedTask for MyTask {
    // Using anyhow for simplicity but could be your Error type
    type Error = anyhow::Error;

    async fn run_forever(&mut self) -> anyhow::Result<()> {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            println!("Task is running!");
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
    let mut supervisor = SupervisorBuilder::default()
        .with_task(MyTask)
        .build();

    supervisor.run_and_supervise().await; // yields if all the tasks are Dead
}
```

The supervisor will:
1. Start all tasks, each running its `run_forever` logic.
2. Send heartbeats every second to confirm task health.
3. Restart tasks that fail or miss heartbeats.

## Contributing

Contributions are welcomed! Please:
1. Fork the repository on GitHub.
2. Submit a pull request with your changes or open an issue for discussion.

## License
This crate is licensed under the MIT License. See the LICENSE file for details.

