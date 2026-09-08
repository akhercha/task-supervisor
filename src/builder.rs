use std::{collections::HashMap, sync::Arc, time::Duration};

use crate::{
    supervisor::{Config, Supervisor},
    task::{Slot, SupervisedTask},
    SupervisorHandle,
};

/// Configures and spawns a supervisor.
///
/// | Setting | Default |
/// | --- | --- |
/// | `max_restart_attempts` | 5 |
/// | `base_restart_delay` | 1s |
/// | `max_restart_delay` | 30s |
/// | `stable_after` | 60s |
/// | `dead_tasks_threshold` | disabled |
/// | `stop_timeout` | 5s |
pub struct SupervisorBuilder {
    tasks: HashMap<Arc<str>, Slot>,
    config: Config,
}

impl SupervisorBuilder {
    /// A builder with the defaults listed above and no tasks.
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            config: Config {
                max_restart_attempts: Some(5),
                base_restart_delay: Duration::from_secs(1),
                max_restart_delay: Duration::from_secs(30),
                stable_after: Duration::from_secs(60),
                dead_tasks_threshold: None,
                stop_timeout: Duration::from_secs(5),
            },
        }
    }

    /// Registers a task to start with the supervisor. A later call with the
    /// same name replaces the earlier task.
    pub fn with_task(mut self, name: &str, task: impl SupervisedTask) -> Self {
        let name: Arc<str> = Arc::from(name);
        self.tasks
            .insert(Arc::clone(&name), Slot::new(name, Box::new(task)));
        self
    }

    /// Number of automatic restarts before a task is declared
    /// [`Dead`](crate::TaskStatus::Dead). `0` disables restarts.
    pub fn with_max_restart_attempts(mut self, attempts: u32) -> Self {
        self.config.max_restart_attempts = Some(attempts);
        self
    }

    /// Never give up restarting a failing task.
    pub fn with_unlimited_restarts(mut self) -> Self {
        self.config.max_restart_attempts = None;
        self
    }

    /// Delay before the first restart. Each further restart doubles it.
    pub fn with_base_restart_delay(mut self, delay: Duration) -> Self {
        self.config.base_restart_delay = delay;
        self
    }

    /// Upper bound for the restart delay.
    pub fn with_max_restart_delay(mut self, delay: Duration) -> Self {
        self.config.max_restart_delay = delay;
        self
    }

    /// A run that lasts at least this long resets the task's restart budget
    /// when it fails.
    pub fn with_stable_after(mut self, duration: Duration) -> Self {
        self.config.stable_after = duration;
        self
    }

    /// Shuts the supervisor down with
    /// [`TooManyDeadTasks`](crate::SupervisorError::TooManyDeadTasks) once at
    /// least one task is dead and `dead / total >= fraction`. Killed tasks
    /// count as dead. `fraction` is clamped to `0.0..=1.0`: `0.0` means "any
    /// dead task", `1.0` means "every task".
    pub fn with_dead_tasks_threshold(mut self, fraction: f64) -> Self {
        self.config.dead_tasks_threshold = Some(fraction.clamp(0.0, 1.0));
        self
    }

    /// How long a cancelled run may keep going before its future is dropped.
    /// Applies to kill, restart and shutdown.
    pub fn with_stop_timeout(mut self, timeout: Duration) -> Self {
        self.config.stop_timeout = timeout;
        self
    }

    /// Spawns the supervisor and every registered task.
    ///
    /// # Panics
    ///
    /// Panics if called outside a Tokio runtime.
    pub fn spawn(self) -> SupervisorHandle {
        Supervisor::spawn(self.tasks, self.config)
    }
}

impl Default for SupervisorBuilder {
    fn default() -> Self {
        Self::new()
    }
}
