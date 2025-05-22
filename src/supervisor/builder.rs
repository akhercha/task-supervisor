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
    health_check_interval: Duration,
    max_restart_attempts: u32,
    base_restart_delay: Duration,
    task_stable_after_delay: Duration,
    max_dead_tasks_percentage_threshold: Option<f64>,
}

impl SupervisorBuilder {
    /// Creates a new builder with default configuration values.
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            health_check_interval: Duration::from_millis(200),
            max_restart_attempts: 5,
            base_restart_delay: Duration::from_secs(1),
            task_stable_after_delay: Duration::from_secs(80),
            max_dead_tasks_percentage_threshold: None,
        }
    }

    /// Adds a task to the supervisor with the specified name.
    pub fn with_task(mut self, name: &str, task: impl CloneableSupervisedTask) -> Self {
        let handle =
            TaskHandle::from_task(task, self.max_restart_attempts, self.base_restart_delay);
        self.tasks.insert(name.into(), handle);
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

    /// Sets the delay after which a task is considered stable and healthy.
    /// When a task is considered stable, its restarts are reset to zero.
    pub fn with_task_being_stable_after(mut self, delay: Duration) -> Self {
        self.task_stable_after_delay = delay;
        self
    }

    /// Sets the threshold for the percentage of dead tasks that will trigger a supervisor shutdown.
    ///
    /// The `threshold_percentage` should be a value between 0.0 (0%) and 1.0 (100%).
    /// If the percentage of dead tasks exceeds this value, the supervisor will shut down
    /// and return an error.
    pub fn with_dead_tasks_threshold(mut self, threshold_percentage: Option<f64>) -> Self {
        self.max_dead_tasks_percentage_threshold = threshold_percentage.map(|t| t.clamp(0.0, 1.0));
        self
    }

    /// Constructs the `Supervisor` with the configured settings.
    pub fn build(self) -> Supervisor {
        let (internal_tx, internal_rx) = mpsc::unbounded_channel();
        let (user_tx, user_rx) = mpsc::unbounded_channel();
        Supervisor {
            tasks: self.tasks,
            health_check_interval: self.health_check_interval,
            base_restart_delay: self.base_restart_delay,
            max_restart_attempts: self.max_restart_attempts,
            task_is_stable_after: self.task_stable_after_delay,
            max_dead_tasks_percentage_threshold: self.max_dead_tasks_percentage_threshold,
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
