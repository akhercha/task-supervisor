# task-supervisor

[![Crates.io](https://img.shields.io/crates/v/task-supervisor.svg)](https://crates.io/crates/task-supervisor)
[![Docs.rs](https://docs.rs/task-supervisor/badge.svg)](https://docs.rs/task-supervisor)

Keeps long-lived Tokio tasks alive: restarts them with exponential backoff when they fail or panic, stops them gracefully, and lets you add, restart, kill or inspect tasks at runtime.

```bash
cargo add task-supervisor
```

## Example

```rust
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

See [`examples/simple.rs`](examples/simple.rs) for restarts, runtime control and tracing output.

## Lifecycle

```text
           add/with_task
                │
                ▼
  ┌────────► Running ──── Ok(()) ────► Completed
  │             │
  │        Err / panic
  │             │
  │             ▼
  │        Restarting ── budget exhausted ──► Dead
  │             │
  └── backoff ──┘

  kill / restart / shutdown on a Running task → Stopping → Dead (or a new Running)
```

* A failed run is restarted after `base_restart_delay * 2^n`, capped at `max_restart_delay`, up to `max_restart_attempts` times. A run that lasted at least `stable_after` resets that budget when it fails.
* Stopping a task cancels its `CancellationToken`; the run then has `stop_timeout` to return before its future is dropped. Tasks that need no cleanup can ignore the token.
* `run` receives a fresh **clone** of the registered task on every start. Owned fields reset; `Arc` fields are shared across runs.
* Dropping the last `SupervisorHandle` shuts the supervisor down gracefully.

## Configuration

| `SupervisorBuilder` method          | Default  | Meaning                                                      |
| ----------------------------------- | -------- | ------------------------------------------------------------ |
| `with_max_restart_attempts(n)`      | 5        | Restarts before a task is `Dead`; `with_unlimited_restarts()` |
| `with_base_restart_delay(d)`        | 1s       | Delay before the first restart, doubled each time            |
| `with_max_restart_delay(d)`         | 30s      | Cap on the restart delay                                     |
| `with_stable_after(d)`              | 60s      | Run length that resets the restart budget                    |
| `with_dead_tasks_threshold(f)`      | disabled | Shut down once a task is dead and `dead / total >= f` (`0.0` = any, `1.0` = all; kills count) |
| `with_stop_timeout(d)`              | 5s       | Grace period after cancellation before a run is dropped (kill, restart, shutdown) |

## Runtime control

`spawn()` returns a `SupervisorHandle` (cheap to clone). Every request resolves once its effect is visible: `add_task` when the task is `Running`, `kill_task` when it is `Dead`, `restart_task` when the new run is `Running`. `kill_task`/`restart_task` on a running task therefore take up to `stop_timeout`.

| Method                       | Errors                        |
| ---------------------------- | ----------------------------- |
| `add_task(name, task).await` | `TaskAlreadyExists`, `Closed` |
| `restart_task(name).await`   | `TaskNotFound`, `Closed`      |
| `kill_task(name).await`      | `TaskNotFound`, `Closed`      |
| `task_status(name).await`    | `TaskNotFound`, `Closed`      |
| `task_statuses().await`      | `Closed`                      |
| `shutdown().await`           | `SupervisorError`             |
| `wait().await`               | `SupervisorError`             |

`wait()` and `shutdown()` return `Err(TooManyDeadTasks)` when the dead-task threshold triggered the shutdown, `Err(Panicked)` if the supervisor itself panicked, and `Err(Aborted)` if the Tokio runtime shut down first.

## Errors

`TaskError` is `Box<dyn Error + Send + Sync>`; anything implementing `std::error::Error` converts with `?`, including `anyhow::Error`. Panics inside `run` are caught and treated as failures, including during cancellation (logged at `warn`).

## Logging

Supervisor activity is emitted through [`tracing`](https://docs.rs/tracing) (`info` for lifecycle events, `warn` for scheduled restarts, `error` for dead tasks). Without a subscriber it costs nothing.

## Upgrading from 0.4

* `SupervisedTask`: `run(&mut self)` → `run(self, cancel: CancellationToken)` (`mut self` to mutate); `Clone` is now a supertrait.
* `build().run()` → `spawn()`; the `Supervisor` type and `TaskName` alias are gone.
* Handle methods are `async` and report errors: `get_task_status` → `task_status` (returns `Err(TaskNotFound)` instead of `None`), `get_all_task_statuses` → `task_statuses` (keys are `Arc<str>`; `statuses["name"]` still works), `restart` → `restart_task`, `kill_task` now waits for the task to stop. `add_task` on an existing name returns `Err(TaskAlreadyExists)` instead of being ignored. `shutdown()` is `async` and returns the supervisor outcome.
* `SupervisorHandleError::{SendError, RecvError}` → `Closed`, plus `TaskAlreadyExists` and `TaskNotFound`.
* `SupervisorError::TooManyDeadTasks { current_percentage, threshold }` → `{ dead, total, threshold }`; new variants `Panicked` and `Aborted`.
* `TaskStatus`: `Healthy` → `Running`, `Failed` → `Restarting`, `Created` removed, `Stopping` added; `is_healthy`/`is_dead`/`is_restarting`/`has_completed` removed (compare variants).
* `with_health_check_interval` removed (no polling anymore); `with_max_backoff_exponent(n)` → `with_max_restart_delay(base * 2^n)`; `with_task_being_stable_after` → `with_stable_after`; `with_dead_tasks_threshold(Some(f))` → `with_dead_tasks_threshold(f)`, now triggers on `>=` once at least one task is dead.
* `anyhow` and `tracing` features removed: `?` on `anyhow::Error` works without a feature; `tracing` is always on.
* Dropping a handle clone no longer shuts the supervisor down; only the last one does.
* Minimum tokio: 1.21 (`JoinSet`).

## License

[MIT](./LICENSE)
