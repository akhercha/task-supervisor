pub(crate) mod builder;
pub(crate) mod handle;

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use tokio::{sync::mpsc, time::interval_at};
use tokio_util::sync::CancellationToken;

use crate::{
    supervisor::handle::{SupervisorHandle, SupervisorMessage},
    task::{TaskHandle, TaskOutcome, TaskStatus},
};

/// Internal messages sent from tasks and by the `Supervisor` to manage task lifecycle.
#[derive(Debug)]
pub(crate) enum SupervisedTaskMessage {
    /// Sent by the supervisor to trigger a task restart.
    Restart(String),
    /// Sent when a task completes, either successfully or with a failure.
    Completed(String, TaskOutcome),
    /// Sent when a shutdown signal is asked by the User. Close every tasks & the supervisor.
    Shutdown,
}

/// Manages a set of tasks, ensuring they remain operational through restarts.
///
/// The `Supervisor` spawns each task with a heartbeat mechanism to monitor liveness.
/// If a task stops sending heartbeats or fails, it is restarted with an exponential backoff.
/// User commands such as adding, restarting, or killing tasks are supported via the `SupervisorHandle`.
pub struct Supervisor {
    // List of current tasks
    tasks: HashMap<String, TaskHandle>,
    // Durations for tasks lifecycle
    timeout_threshold: Duration,
    health_check_initial_delay: Duration,
    health_check_interval: Duration,
    base_restart_delay: Duration,
    task_is_stable_after: Duration,
    max_restart_attempts: u32,
    // Channels between the User & the Supervisor
    external_tx: mpsc::UnboundedSender<SupervisorMessage>,
    external_rx: mpsc::UnboundedReceiver<SupervisorMessage>,
    // Internal channels used in the Supervisor for actions propagation
    internal_tx: mpsc::UnboundedSender<SupervisedTaskMessage>,
    internal_rx: mpsc::UnboundedReceiver<SupervisedTaskMessage>,
}

impl Supervisor {
    /// Runs the supervisor, consuming it and returning a handle for external control.
    ///
    /// This method initiates all tasks and starts the supervision loop.
    pub fn run(self) -> SupervisorHandle {
        let user_tx = self.external_tx.clone();
        let handle = tokio::spawn(async move {
            self.run_and_supervise().await;
        });
        SupervisorHandle::new(handle, user_tx)
    }

    /// Starts and supervises all tasks, running the main supervision loop.
    async fn run_and_supervise(mut self) {
        self.start_all_tasks().await;
        self.supervise_all_tasks().await;
    }

    /// Initiates all tasks managed by the supervisor.
    async fn start_all_tasks(&mut self) {
        for (task_name, task_handle) in self.tasks.iter_mut() {
            Self::start_task(task_name.to_string(), task_handle, self.internal_tx.clone()).await;
        }
    }

    /// Supervises tasks by processing messages and performing periodic health checks.
    async fn supervise_all_tasks(&mut self) {
        let start_time = tokio::time::Instant::now() + self.health_check_initial_delay;
        let mut health_check_ticker = interval_at(start_time, self.health_check_interval);

        loop {
            tokio::select! {
                biased; // Process internal messages with higher priority if many are pending
                Some(internal_msg) = self.internal_rx.recv() => {
                    if matches!(internal_msg, SupervisedTaskMessage::Shutdown) {
                        return;
                    }
                    self.handle_internal_message(internal_msg).await;
                },
                Some(user_msg) = self.external_rx.recv() => {
                    self.handle_user_message(user_msg).await;
                },
                _ = health_check_ticker.tick() => {
                    self.check_all_health().await;
                }
            }
        }
    }

    async fn handle_internal_message(&mut self, msg: SupervisedTaskMessage) {
        match msg {
            SupervisedTaskMessage::Restart(task_name) => {
                self.restart_task(task_name).await;
            }
            SupervisedTaskMessage::Completed(task_name, outcome) => {
                self.handle_task_completion(task_name, outcome).await;
            }
            SupervisedTaskMessage::Shutdown => {
                unreachable!("Shutdown should be handled by the main select loop to break.");
            }
        }
    }

    /// Processes user commands received via the `SupervisorHandle`.
    async fn handle_user_message(&mut self, msg: SupervisorMessage) {
        match msg {
            SupervisorMessage::AddTask(task_name, task_dyn) => {
                if self.tasks.contains_key(&task_name) {
                    return;
                }
                let mut task_handle =
                    TaskHandle::new(task_dyn, self.max_restart_attempts, self.base_restart_delay);
                Self::start_task(
                    task_name.clone(),
                    &mut task_handle,
                    self.internal_tx.clone(),
                )
                .await;
                self.tasks.insert(task_name, task_handle);
            }
            SupervisorMessage::RestartTask(task_name) => {
                self.restart_task(task_name).await;
            }
            SupervisorMessage::KillTask(task_name) => {
                if let Some(task_handle) = self.tasks.get_mut(&task_name) {
                    if task_handle.status != TaskStatus::Dead {
                        task_handle.mark(TaskStatus::Dead); // Mark as Dead first
                        task_handle.clean().await; // Then clean up its resources
                    }
                }
            }
            SupervisorMessage::GetTaskStatus(task_name, sender) => {
                let status = self.tasks.get(&task_name).map(|handle| handle.status);
                let _ = sender.send(status);
            }
            SupervisorMessage::GetAllTaskStatuses(sender) => {
                let statuses = self
                    .tasks
                    .iter()
                    .map(|(name, handle)| (name.clone(), handle.status))
                    .collect();
                let _ = sender.send(statuses);
            }
            SupervisorMessage::Shutdown => {
                for (_, task_handle) in self.tasks.iter_mut() {
                    if task_handle.status != TaskStatus::Dead
                        && task_handle.status != TaskStatus::Completed
                    {
                        task_handle.clean().await;
                        task_handle.mark(TaskStatus::Dead);
                    }
                }
                let _ = self.internal_tx.send(SupervisedTaskMessage::Shutdown);
            }
        }
    }

    /// Starts a task, setting up its execution, heartbeat, and completion handling.
    async fn start_task(
        task_name: String,
        task_handle: &mut TaskHandle,
        internal_tx: mpsc::UnboundedSender<SupervisedTaskMessage>,
    ) {
        task_handle.started_at = Some(Instant::now());
        task_handle.mark(TaskStatus::Starting);

        let token = CancellationToken::new();
        task_handle.cancellation_token = Some(token.clone());

        let (completion_tx, mut completion_rx) = mpsc::channel::<TaskOutcome>(1);

        // Completion Listener Task
        let task_name_completion = task_name.clone();
        let token_completion = token.clone();
        let internal_tx_completion = internal_tx.clone();
        let completion_listener_handle = tokio::spawn(async move {
            tokio::select! {
                _ = token_completion.cancelled() => { }
                Some(outcome) = completion_rx.recv() => {
                    let completion_msg = SupervisedTaskMessage::Completed(task_name_completion.clone(), outcome);
                    let _ = internal_tx_completion.send(completion_msg);
                    token_completion.cancel();
                }
            }
        });

        // Main Task Execution
        let mut task_instance = task_handle.task.clone_box();
        let token_main = token.clone();
        let main_task_execution_handle = tokio::spawn(async move {
            tokio::select! {
                _ = token_main.cancelled() => { }
                run_result = task_instance.run() => {
                    match run_result {
                        Ok(outcome) => {
                            let _ = completion_tx.send(outcome).await;
                        }
                        Err(e) => {
                            let _ = completion_tx.send(TaskOutcome::Failed(e.to_string())).await;
                        }
                    }
                }
            }
        });

        task_handle.main_task_join_handle = Some(main_task_execution_handle);
        task_handle.auxiliary_join_handles = vec![completion_listener_handle];
    }

    /// Restarts a task after cleaning up its previous execution.
    async fn restart_task(&mut self, task_name: String) {
        if let Some(task_handle) = self.tasks.get_mut(&task_name) {
            task_handle.clean().await;
            task_handle.mark(TaskStatus::Starting);
            Self::start_task(task_name, task_handle, self.internal_tx.clone()).await;
        }
    }

    async fn check_all_health(&mut self) {
        let mut tasks_needing_restart_check: Vec<String> = Vec::new();
        let now = Instant::now();

        for (task_name, task_handle) in self.tasks.iter_mut() {
            match task_handle.status {
                TaskStatus::Starting => {
                    if let Some(started_at) = task_handle.started_at {
                        if now.duration_since(started_at) > self.timeout_threshold {
                            task_handle.mark(TaskStatus::Failed);
                            tasks_needing_restart_check.push(task_name.clone());
                            continue;
                        }
                    }
                    if let Some(main_handle) = &task_handle.main_task_join_handle {
                        if main_handle.is_finished() {
                            task_handle.mark(TaskStatus::Failed);
                            tasks_needing_restart_check.push(task_name.clone());
                        } else {
                            task_handle.mark(TaskStatus::Healthy);
                            task_handle.healthy_since = Some(now);
                        }
                    } else {
                        task_handle.mark(TaskStatus::Failed);
                        tasks_needing_restart_check.push(task_name.clone());
                    }
                }
                TaskStatus::Healthy => {
                    if let Some(main_handle) = &task_handle.main_task_join_handle {
                        if main_handle.is_finished() {
                            task_handle.mark(TaskStatus::Failed);
                            tasks_needing_restart_check.push(task_name.clone());
                        } else {
                            // Task is Healthy and running. Check for stability.
                            if let Some(healthy_since) = task_handle.healthy_since {
                                if now.duration_since(healthy_since) > self.task_is_stable_after
                                    && task_handle.restart_attempts > 0
                                {
                                    task_handle.restart_attempts = 0;
                                }
                            } else {
                                task_handle.healthy_since = Some(now);
                            }
                        }
                    } else {
                        task_handle.mark(TaskStatus::Failed);
                        tasks_needing_restart_check.push(task_name.clone());
                    }
                }
                _ => {}
            }
        }

        for task_name_to_check in tasks_needing_restart_check {
            if let Some(task_handle) = self.tasks.get_mut(&task_name_to_check) {
                // This task was marked Failed by the loop above.
                // Now apply restart policy.
                if task_handle.status == TaskStatus::Failed {
                    // Ensure it's still failed
                    if task_handle.has_exceeded_max_retries() {
                        task_handle.mark(TaskStatus::Dead);
                        task_handle.clean().await;
                        continue;
                    }

                    // Increment before calculating delay for the current attempt
                    task_handle.restart_attempts = task_handle.restart_attempts.saturating_add(1);
                    let restart_delay = task_handle.restart_delay();

                    let internal_tx_clone = self.internal_tx.clone();
                    // Clean is called by restart_task, so not needed here if sending Restart message
                    tokio::spawn(async move {
                        tokio::time::sleep(restart_delay).await;
                        let _ = internal_tx_clone
                            .send(SupervisedTaskMessage::Restart(task_name_to_check.clone()));
                    });
                }
            }
        }
    }

    async fn handle_task_completion(&mut self, task_name: String, outcome: TaskOutcome) {
        let Some(task_handle) = self.tasks.get_mut(&task_name) else {
            return;
        };

        task_handle.main_task_join_handle.take();
        for handle in task_handle.auxiliary_join_handles.drain(..) {
            handle.abort();
        }
        if let Some(token) = task_handle.cancellation_token.take() {
            token.cancel();
        }
        task_handle.healthy_since = None;

        match outcome {
            TaskOutcome::Completed => {
                task_handle.mark(TaskStatus::Completed);
            }
            TaskOutcome::Failed(_) => {
                task_handle.mark(TaskStatus::Failed);

                if task_handle.has_exceeded_max_retries() {
                    task_handle.mark(TaskStatus::Dead);
                    return;
                }

                task_handle.restart_attempts = task_handle.restart_attempts.saturating_add(1);
                let restart_delay = task_handle.restart_delay();

                let internal_tx_clone = self.internal_tx.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(restart_delay).await;
                    let _ =
                        internal_tx_clone.send(SupervisedTaskMessage::Restart(task_name.clone()));
                });
            }
        }
    }
}
