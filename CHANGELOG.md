# Changelog

## 0.5.0

Rewrite of the supervisor core. Event-driven (no periodic health check), cooperative cancellation, real errors from the handle.

### Fixed

* Dropping any `SupervisorHandle` clone shut the supervisor down. Now only the last one does.
* A pending backoff restart could resurrect a killed task or double-start a manually restarted one.
* Panics in `run` were detected by polling and their message lost. They are now caught and reported as failures.
* `TooManyDeadTasks` printed a fraction with a `%` sign.
* A supervisor panic was reported as `Ok(())` by `wait()`.

### Breaking

* `SupervisedTask::run(&mut self)` is now `run(self, cancel: CancellationToken)`. Use `mut self` to mutate. `Clone` is a supertrait.
* `build().run()` is now `spawn()`. The `Supervisor` type and `TaskName` alias are gone.
* Handle methods are `async` and return errors: `get_task_status` → `task_status` (`Err(TaskNotFound)` instead of `None`), `get_all_task_statuses` → `task_statuses` (keys are `Arc<str>`), `restart` → `restart_task`. `add_task` on an existing name returns `Err(TaskAlreadyExists)` instead of being ignored. `kill_task` and `restart_task` resolve once the task has stopped or restarted. `shutdown()` is `async` and returns the supervisor outcome.
* `SupervisorHandleError::{SendError, RecvError}` → `Closed`; new `TaskAlreadyExists`, `TaskNotFound`.
* `SupervisorError::TooManyDeadTasks { current_percentage, threshold }` → `{ dead, total, threshold }`; new `Panicked`, `Aborted`.
* `TaskStatus`: `Healthy` → `Running`, `Failed` → `Restarting`, `Created` removed, `Stopping` added. `is_healthy` / `is_dead` / `is_restarting` / `has_completed` removed.
* Builder: `with_max_restart_attempts(n)` and `with_task_being_stable_after(d)` → `with_restart_limit(n, window)` (at most `n` restarts in any `window`, default 5 in 60s). `with_max_backoff_exponent(n)` → `with_max_restart_delay(d)`. `with_health_check_interval` removed. `with_dead_tasks_threshold(Some(f))` → `with_dead_tasks_threshold(f)`, triggers on `>=` once a task is dead. New `with_stop_timeout(d)`.
* `anyhow` and `tracing` cargo features removed. `?` on `anyhow::Error` works without a feature; `tracing` is always on.
* Minimum tokio 1.21.
