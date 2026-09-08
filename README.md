# task-supervisor

[![Crates.io](https://img.shields.io/crates/v/task-supervisor.svg)](https://crates.io/crates/task-supervisor)
[![Docs.rs](https://docs.rs/task-supervisor/badge.svg)](https://docs.rs/task-supervisor)

Keeps long-lived Tokio tasks alive. Restarts them with exponential backoff when they fail or panic, stops them cleanly, and lets you add, restart, kill or inspect them at runtime.

```bash
cargo add task-supervisor
```

## Example

```rust,no_run
use std::time::Duration;
use task_supervisor::{CancellationToken, SupervisedTask, SupervisorBuilder, TaskResult};

#[derive(Clone)]
struct Heartbeat;

impl SupervisedTask for Heartbeat {
    async fn run(self, cancel: CancellationToken) -> TaskResult {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return Ok(()),
                _ = tokio::time::sleep(Duration::from_secs(1)) => println!("beat"),
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let handle = SupervisorBuilder::new()
        .with_task("heartbeat", Heartbeat)
        .spawn();

    tokio::signal::ctrl_c().await.unwrap();
    handle.shutdown().await.unwrap();
}
```

[`examples/simple.rs`](https://github.com/akhercha/task-supervisor/blob/main/examples/simple.rs) shows restarts, runtime control and tracing output.

## How it works

```text
           add_task / with_task
                    │
                    ▼
  ┌────────────► Running ──── Ok(()) ────► Completed
  │                 │
  │            Err / panic
  │                 │
  │                 ▼
  │            Restarting ── limit reached ──► Dead
  │                 │
  └─── backoff ─────┘

  kill / restart / shutdown on a Running task: Stopping, then Dead or a new Running
```

* Each run gets a fresh clone of the registered task. Owned fields reset on every run; `Arc` fields are shared.
* A failed run restarts after `base_restart_delay * 2^n`, capped at `max_restart_delay`, where `n` is the number of restarts in the current `restart_limit` window. One restart too many and the task is `Dead`.
* Stopping a task cancels its `CancellationToken`. The run has `stop_timeout` to return; then its future is dropped. Tasks without cleanup can ignore the token.
* Panics inside `run` are caught and count as failures.
* Dropping the last `SupervisorHandle` shuts the supervisor down.

## Configuration

| `SupervisorBuilder` method      | Default  | Meaning |
| ------------------------------- | -------- | ------- |
| `with_restart_limit(n, window)` | 5 in 60s | Max restarts within any `window`. `with_unlimited_restarts()` removes the limit. |
| `with_base_restart_delay(d)`    | 1s       | Delay before the first restart in a window; doubles each time. |
| `with_max_restart_delay(d)`     | 30s      | Cap on the restart delay. |
| `with_stop_timeout(d)`          | 5s       | Time a cancelled run gets before being dropped. |
| `with_dead_tasks_threshold(f)`  | off      | Shut down once `dead / total >= f` and at least one task is dead. Kills count. |

## Runtime control

`spawn()` returns a `SupervisorHandle`, cheap to clone. Each request resolves when its effect is visible: `add_task` when the task is `Running`, `kill_task` when it is `Dead`, `restart_task` when the new run is `Running`. On a running task, `kill_task` and `restart_task` take up to `stop_timeout`.

| Method                       | Errors |
| ---------------------------- | ------ |
| `add_task(name, task).await` | `TaskAlreadyExists`, `Closed` |
| `restart_task(name).await`   | `TaskNotFound`, `Closed` |
| `kill_task(name).await`      | `TaskNotFound`, `Closed` |
| `task_status(name).await`    | `TaskNotFound`, `Closed` |
| `task_statuses().await`      | `Closed` |
| `shutdown().await`           | `TooManyDeadTasks`, `Panicked`, `Aborted` |
| `wait().await`               | `TooManyDeadTasks`, `Panicked`, `Aborted` |

`Closed`: the supervisor has exited. `Aborted`: the Tokio runtime shut down under it. `Panicked`: a bug in this crate.

## Errors and logs

`TaskError` is `Box<dyn Error + Send + Sync>`, so `?` works on any `std::error::Error`, `anyhow::Error` included.

Lifecycle events go through [`tracing`](https://docs.rs/tracing): `info` for start/stop, `warn` for scheduled restarts and errors during cancellation, `error` for dead tasks. No subscriber, no cost.

Breaking changes are listed in the [changelog](https://github.com/akhercha/task-supervisor/blob/main/CHANGELOG.md).

## License

[MIT](./LICENSE)
