# task-supervisor

[![Crates.io](https://img.shields.io/crates/v/supervisor.svg)](https://crates.io/crates/task-supervisor)
[![Docs.rs](https://docs.rs/supervisor/badge.svg)](https://docs.rs/task-supervisor)

`task-supervisor` helps you keep Tokio tasks alive.
It watches each task, restarts it if it crashes or stops responding, and lets you add, restart, or kill tasks at runtime.

## Install

```bash
cargo add task-supervisor
```

## Quick example

```rust
use task_supervisor::{SupervisorBuilder, SupervisedTask, TaskResult};

#[derive(Clone)]
struct Printer;

impl SupervisedTask for Printer {
    async fn run(&mut self) -> TaskResult {w
        println!("hello");
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    let supervisor = SupervisorBuilder::default()
        .with_task("printer", Printer)
        .build()
        .run();

    supervisor.wait().await.unwrap();   // wait until every task finishes or is killed
}
```

## What you get

* **Automatic restarts** – failed tasks are relaunched with exponential back-off.
* **Dynamic control** – add, restart, kill, or query tasks at runtime through a `SupervisorHandle`.
* **Configurable** – health-check interval, restart limits, back-off, dead-task threshold.

## Usage

Build a supervisor with `SupervisorBuilder`, call `.build()` to get a `Supervisor`, then `.run()` to start it. This returns a `SupervisorHandle` you use to control things at runtime:

| Method                          | Description                      |
| ------------------------------- | -------------------------------- |
| `wait().await`                  | Block until the supervisor exits |
| `add_task(name, task)`          | Register and start a new task    |
| `restart(name)`                 | Force-restart a task             |
| `kill_task(name)`               | Stop a task permanently          |
| `get_task_status(name).await`   | Get a task's `TaskStatus`        |
| `get_all_task_statuses().await` | Get every task's status          |
| `shutdown()`                    | Stop all tasks and exit          |

The handle auto-shuts down the supervisor when all clones are dropped.

## Clone and restart behaviour

Every task must implement `Clone`. The supervisor stores the **original** instance and clones it each time the task is started or restarted. Mutations made through `&mut self` in `run()` only affect the running clone and are lost on restart.

To share state across restarts, wrap it in an `Arc` (e.g. `Arc<AtomicUsize>`). Plain owned fields will always start from their original value. See the [`SupervisedTask`](https://docs.rs/task-supervisor/latest/task_supervisor/trait.SupervisedTask.html) docs for a full example.

## License

[MIT](./LICENSE)
