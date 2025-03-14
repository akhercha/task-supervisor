use std::{collections::HashMap, time::Duration};

use tokio::sync::mpsc;

use crate::{
    task::{CloneableSupervisedTask, TaskHandle},
    Supervisor,
};

/// Builds a `Supervisor` instance with configurable parameters.
///
/// Allows customization of task timeout, heartbeat interval, health check timing,
/// and per-task restart settings.
pub struct SupervisorBuilder {
    tasks: HashMap<String, TaskHandle>,
    timeout_threshold: Duration,
    heartbeat_interval: Duration,
    health_check_initial_delay: Duration,
    health_check_interval: Duration,
    max_restart_attempts: u32,
    base_restart_delay: Duration,
}

impl SupervisorBuilder {
    /// Creates a new builder with default configuration values.
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            timeout_threshold: Duration::from_secs(2),
            heartbeat_interval: Duration::from_millis(100),
            health_check_initial_delay: Duration::from_secs(3),
            health_check_interval: Duration::from_millis(200),
            max_restart_attempts: 5,
            base_restart_delay: Duration::from_secs(1),
        }
    }

    /// Adds a task to the supervisor with the specified name.
    pub fn with_task(mut self, name: &str, task: impl CloneableSupervisedTask) -> Self {
        let task_handle =
            TaskHandle::new_with_config(task, self.max_restart_attempts, self.base_restart_delay);
        self.tasks.insert(name.into(), task_handle);
        self
    }

    /// Sets the timeout threshold for detecting task crashes.
    pub fn with_timeout_threshold(mut self, threshold: Duration) -> Self {
        self.timeout_threshold = threshold;
        self
    }

    /// Sets the interval at which tasks send heartbeats.
    pub fn with_heartbeat_interval(mut self, interval: Duration) -> Self {
        self.heartbeat_interval = interval;
        self
    }

    /// Sets the initial delay before health checks begin.
    pub fn with_health_check_initial_delay(mut self, delay: Duration) -> Self {
        self.health_check_initial_delay = delay;
        self
    }

    /// Sets the interval between health checks.
    pub fn with_health_check_interval(mut self, interval: Duration) -> Self {
        self.health_check_interval = interval;
        self
    }

    /// Sets the maximum number of restart attempts for tasks.
    pub fn with_max_restart_attempts(mut self, attempts: u32) -> Self {
        self.max_restart_attempts = attempts;
        self
    }

    /// Sets the base delay for task restarts, used in exponential backoff.
    pub fn with_base_restart_delay(mut self, delay: Duration) -> Self {
        self.base_restart_delay = delay;
        self
    }

    /// Constructs the `Supervisor` with the configured settings.
    pub fn build(self) -> Supervisor {
        let (internal_tx, internal_rx) = mpsc::unbounded_channel();
        let (user_tx, user_rx) = mpsc::unbounded_channel();
        Supervisor {
            tasks: self.tasks,
            timeout_threshold: self.timeout_threshold,
            heartbeat_interval: self.heartbeat_interval,
            health_check_initial_delay: self.health_check_initial_delay,
            health_check_interval: self.health_check_interval,
            internal_tx,
            internal_rx,
            external_tx: user_tx,
            external_rx: user_rx,
        }
    }
}

impl Default for SupervisorBuilder {
    fn default() -> Self {
        Self::new()
    }
}
