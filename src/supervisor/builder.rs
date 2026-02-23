use std::{collections::HashMap, sync::Arc, time::Duration};

use tokio::sync::mpsc;

use crate::{task::TaskHandle, SupervisedTask, Supervisor};

/// Builds a `Supervisor` with configurable parameters.
///
/// `with_task()` and configuration methods can be called in any order;
/// task restart settings are applied uniformly at `build()` time.
pub struct SupervisorBuilder {
    tasks: HashMap<Arc<str>, TaskHandle>,
    health_check_interval: Duration,
    max_restart_attempts: Option<u32>,
    base_restart_delay: Duration,
    max_backoff_exponent: u32,
    task_stable_after_delay: Duration,
    max_dead_tasks_percentage_threshold: Option<f64>,
}

impl SupervisorBuilder {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            health_check_interval: Duration::from_millis(200),
            max_restart_attempts: Some(5),
            base_restart_delay: Duration::from_secs(1),
            max_backoff_exponent: 5,
            task_stable_after_delay: Duration::from_secs(80),
            max_dead_tasks_percentage_threshold: None,
        }
    }

    pub fn with_task(mut self, name: &str, task: impl SupervisedTask + Clone) -> Self {
        let handle = TaskHandle::from_task(task);
        self.tasks.insert(Arc::from(name), handle);
        self
    }

    /// Caps the exponent in the backoff formula:
    /// `delay = base_restart_delay * 2^min(attempt, max_backoff_exponent)`.
    pub fn with_max_backoff_exponent(mut self, exponent: u32) -> Self {
        self.max_backoff_exponent = exponent;
        self
    }

    pub fn with_health_check_interval(mut self, interval: Duration) -> Self {
        self.health_check_interval = interval;
        self
    }

    pub fn with_max_restart_attempts(mut self, attempts: u32) -> Self {
        self.max_restart_attempts = Some(attempts);
        self
    }

    pub fn with_unlimited_restarts(mut self) -> Self {
        self.max_restart_attempts = None;
        self
    }

    pub fn with_base_restart_delay(mut self, delay: Duration) -> Self {
        self.base_restart_delay = delay;
        self
    }

    /// Once a task has been healthy for this long, its restart counter resets to zero.
    pub fn with_task_being_stable_after(mut self, delay: Duration) -> Self {
        self.task_stable_after_delay = delay;
        self
    }

    /// Shuts down the supervisor if the fraction of dead tasks exceeds
    /// `threshold_percentage` (0.0 – 1.0). `None` disables the check.
    pub fn with_dead_tasks_threshold(mut self, threshold_percentage: Option<f64>) -> Self {
        self.max_dead_tasks_percentage_threshold = threshold_percentage.map(|t| t.clamp(0.0, 1.0));
        self
    }

    pub fn build(mut self) -> Supervisor {
        for task_handle in self.tasks.values_mut() {
            task_handle.max_restart_attempts = self.max_restart_attempts;
            task_handle.base_restart_delay = self.base_restart_delay;
            task_handle.max_backoff_exponent = self.max_backoff_exponent;
        }

        let (internal_tx, internal_rx) = mpsc::unbounded_channel();
        let (user_tx, user_rx) = mpsc::unbounded_channel();
        Supervisor {
            tasks: self.tasks,
            health_check_interval: self.health_check_interval,
            base_restart_delay: self.base_restart_delay,
            max_restart_attempts: self.max_restart_attempts,
            max_backoff_exponent: self.max_backoff_exponent,
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
