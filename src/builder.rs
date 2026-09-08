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
/// | `restart_limit` | 5 restarts in 60s |
/// | `base_restart_delay` | 1s |
/// | `max_restart_delay` | 30s |
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
                max_restarts: Some(5),
                restart_window: Duration::from_secs(60),
                base_restart_delay: Duration::from_secs(1),
                max_restart_delay: Duration::from_secs(30),
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

    /// A task that would need more than `max_restarts` restarts within any
    /// `window` is declared [`Dead`](crate::TaskStatus::Dead) instead.
    /// `max_restarts = 0` disables restarts. The backoff delay doubles with
    /// each restart inside the window, so a task that stays up longer than
    /// `window` starts again from `base_restart_delay`.
    pub fn with_restart_limit(mut self, max_restarts: u32, window: Duration) -> Self {
        self.config.max_restarts = Some(max_restarts);
        self.config.restart_window = window;
        self
    }

    /// Never give up restarting a failing task. The window still drives the
    /// backoff delay.
    pub fn with_unlimited_restarts(mut self) -> Self {
        self.config.max_restarts = None;
        self
    }

    /// Delay before the first restart in a window. Each further restart in
    /// the window doubles it.
    pub fn with_base_restart_delay(mut self, delay: Duration) -> Self {
        self.config.base_restart_delay = delay;
        self
    }

    /// Upper bound for the restart delay.
    pub fn with_max_restart_delay(mut self, delay: Duration) -> Self {
        self.config.max_restart_delay = delay;
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
