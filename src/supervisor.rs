use std::{
    collections::{BinaryHeap, HashMap},
    future::{poll_fn, Future},
    ops::ControlFlow,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::Arc,
    task::Poll,
    time::Duration,
};

use tokio::{
    sync::mpsc,
    task::JoinSet,
    time::{sleep_until, timeout, Instant},
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::{
    handle::{Message, SupervisorHandle, SupervisorHandleError},
    task::{panic_message, Slot, TaskResult, TaskStatus},
};

/// Why the supervisor exited abnormally. Returned by
/// [`SupervisorHandle::wait`] and [`SupervisorHandle::shutdown`].
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SupervisorError {
    /// The dead-task threshold configured with
    /// [`with_dead_tasks_threshold`](crate::SupervisorBuilder::with_dead_tasks_threshold)
    /// was reached; every remaining task was stopped.
    #[error("too many dead tasks: {dead}/{total} ({:.0}%) >= threshold {:.0}%", *.dead as f64 / *.total as f64 * 100.0, .threshold * 100.0)]
    TooManyDeadTasks {
        /// Tasks in [`TaskStatus::Dead`] when the threshold was reached.
        dead: usize,
        /// Tasks registered at that moment.
        total: usize,
        /// Configured threshold, as a fraction.
        threshold: f64,
    },
    /// The supervisor task itself panicked (a bug in this crate).
    #[error("supervisor panicked: {0}")]
    Panicked(String),
    /// The Tokio runtime shut down before the supervisor exited.
    #[error("supervisor aborted by runtime shutdown")]
    Aborted,
}

pub(crate) type Outcome = Result<(), SupervisorError>;

#[derive(Clone, Copy)]
pub(crate) struct Config {
    pub(crate) max_restarts: Option<u32>,
    pub(crate) restart_window: Duration,
    pub(crate) base_restart_delay: Duration,
    pub(crate) max_restart_delay: Duration,
    pub(crate) dead_tasks_threshold: Option<f64>,
    pub(crate) stop_timeout: Duration,
}

/// Scheduled restart, ordered earliest-deadline-first for `BinaryHeap`.
struct Timer {
    deadline: Instant,
    name: Arc<str>,
    generation: u64,
}

impl PartialEq for Timer {
    fn eq(&self, other: &Self) -> bool {
        self.deadline == other.deadline
    }
}

impl Eq for Timer {}

impl PartialOrd for Timer {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Timer {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.deadline.cmp(&self.deadline)
    }
}

type RunOutput = (Arc<str>, TaskResult);

pub(crate) struct Supervisor {
    tasks: HashMap<Arc<str>, Slot>,
    config: Config,
    runs: JoinSet<RunOutput>,
    timers: BinaryHeap<Timer>,
}

impl Supervisor {
    pub(crate) fn spawn(tasks: HashMap<Arc<str>, Slot>, config: Config) -> SupervisorHandle {
        let supervisor = Self {
            tasks,
            config,
            runs: JoinSet::new(),
            timers: BinaryHeap::new(),
        };
        let (tx, rx) = mpsc::unbounded_channel();
        let join = tokio::spawn(supervisor.supervise(rx));
        SupervisorHandle::new(join, tx)
    }

    async fn supervise(mut self, mut rx: mpsc::UnboundedReceiver<Message>) -> Outcome {
        for slot in self.tasks.values_mut() {
            start_run(slot, &self.config, &mut self.runs);
        }

        let outcome = loop {
            let deadline = self.timers.peek().map(|t| t.deadline);
            let next_timer = async move {
                match deadline {
                    Some(deadline) => sleep_until(deadline).await,
                    None => std::future::pending().await,
                }
            };

            let flow = tokio::select! {
                biased;
                Some(exit) = self.runs.join_next(), if !self.runs.is_empty() => match exit {
                    Ok((name, result)) => self.on_run_exit(&name, result),
                    // Only happens when the runtime aborts tasks while shutting down.
                    Err(_) => ControlFlow::Continue(()),
                },
                _ = next_timer => {
                    if let Some(timer) = self.timers.pop() {
                        self.on_timer(timer);
                    }
                    ControlFlow::Continue(())
                }
                message = rx.recv() => match message {
                    Some(message) => self.on_message(message),
                    None => {
                        info!("all handles dropped, shutting down");
                        ControlFlow::Break(Ok(()))
                    }
                },
            };

            if let ControlFlow::Break(outcome) = flow {
                break outcome;
            }
        };

        // Requests sent from now on fail with `Closed` instead of waiting for the drain.
        drop(rx);
        self.stop_all().await;
        outcome
    }

    fn on_message(&mut self, message: Message) -> ControlFlow<Outcome> {
        match message {
            Message::AddTask { name, task, reply } => {
                let result = if self.tasks.contains_key(&*name) {
                    Err(SupervisorHandleError::TaskAlreadyExists(name.to_string()))
                } else {
                    let slot = self
                        .tasks
                        .entry(Arc::clone(&name))
                        .or_insert_with(|| Slot::new(name, task));
                    start_run(slot, &self.config, &mut self.runs);
                    Ok(())
                };
                let _ = reply.send(result);
            }
            Message::RestartTask { name, reply } => {
                let Some(slot) = self.tasks.get_mut(name.as_str()) else {
                    let _ = reply.send(Err(SupervisorHandleError::TaskNotFound(name)));
                    return ControlFlow::Continue(());
                };
                info!(task = %name, "restart requested");
                slot.restarts.clear();
                match slot.status {
                    TaskStatus::Running | TaskStatus::Stopping => {
                        slot.request_stop(true);
                        slot.stop_waiters.push(reply);
                    }
                    TaskStatus::Restarting | TaskStatus::Completed | TaskStatus::Dead => {
                        start_run(slot, &self.config, &mut self.runs);
                        let _ = reply.send(Ok(()));
                    }
                }
            }
            Message::KillTask { name, reply } => {
                let Some(slot) = self.tasks.get_mut(name.as_str()) else {
                    let _ = reply.send(Err(SupervisorHandleError::TaskNotFound(name)));
                    return ControlFlow::Continue(());
                };
                info!(task = %name, "kill requested");
                match slot.status {
                    TaskStatus::Running | TaskStatus::Stopping => {
                        slot.request_stop(false);
                        slot.stop_waiters.push(reply);
                    }
                    TaskStatus::Restarting | TaskStatus::Completed => {
                        slot.status = TaskStatus::Dead;
                        let _ = reply.send(Ok(()));
                        return self.check_threshold();
                    }
                    TaskStatus::Dead => {
                        let _ = reply.send(Ok(()));
                    }
                }
            }
            Message::TaskStatus { name, reply } => {
                let result = self
                    .tasks
                    .get(name.as_str())
                    .map(|slot| slot.status)
                    .ok_or(SupervisorHandleError::TaskNotFound(name));
                let _ = reply.send(result);
            }
            Message::TaskStatuses { reply } => {
                let statuses = self
                    .tasks
                    .iter()
                    .map(|(name, slot)| (Arc::clone(name), slot.status))
                    .collect();
                let _ = reply.send(statuses);
            }
            Message::Shutdown => {
                info!("shutdown requested");
                return ControlFlow::Break(Ok(()));
            }
        }
        ControlFlow::Continue(())
    }

    fn on_run_exit(&mut self, name: &str, result: TaskResult) -> ControlFlow<Outcome> {
        let Some(slot) = self.tasks.get_mut(name) else {
            return ControlFlow::Continue(());
        };
        slot.cancel = None;

        if slot.status == TaskStatus::Stopping {
            match &result {
                Ok(()) => debug!(task = name, "stopped"),
                Err(err) => warn!(task = name, error = %err, "stopped with error"),
            }
            let died = !slot.restart_after_stop;
            if died {
                slot.status = TaskStatus::Dead;
            } else {
                start_run(slot, &self.config, &mut self.runs);
            }
            for waiter in std::mem::take(&mut slot.stop_waiters) {
                let _ = waiter.send(Ok(()));
            }
            return if died {
                self.check_threshold()
            } else {
                ControlFlow::Continue(())
            };
        }

        let Err(err) = result else {
            info!(task = name, "completed");
            slot.status = TaskStatus::Completed;
            return ControlFlow::Continue(());
        };

        let now = Instant::now();
        let window = self.config.restart_window;
        while slot
            .restarts
            .front()
            .is_some_and(|at| now.duration_since(*at) >= window)
        {
            slot.restarts.pop_front();
        }
        let recent = slot.restarts.len();
        if self
            .config
            .max_restarts
            .is_some_and(|max| recent >= max as usize)
        {
            error!(
                task = name,
                error = %err,
                restarts = recent,
                window = ?window,
                "failed; restart limit reached, task is dead"
            );
            slot.status = TaskStatus::Dead;
            return self.check_threshold();
        }

        // ponytail: no jitter; add if many tasks share a failing dependency.
        let factor = 2u32.saturating_pow(recent.min(31) as u32);
        let delay = self
            .config
            .base_restart_delay
            .saturating_mul(factor)
            .min(self.config.max_restart_delay);
        slot.restarts.push_back(now);
        // Unlimited mode: the exponent saturates anyway, keep the deque bounded.
        if slot.restarts.len() > 32 {
            slot.restarts.pop_front();
        }
        slot.status = TaskStatus::Restarting;
        warn!(
            task = name,
            error = %err,
            restarts_in_window = recent + 1,
            delay = ?delay,
            "failed; restart scheduled"
        );
        self.timers.push(Timer {
            deadline: Instant::now().checked_add(delay).unwrap_or_else(far_future),
            name: Arc::clone(&slot.name),
            generation: slot.generation,
        });
        ControlFlow::Continue(())
    }

    fn on_timer(&mut self, timer: Timer) {
        let Some(slot) = self.tasks.get_mut(&timer.name) else {
            return;
        };
        if slot.status == TaskStatus::Restarting && slot.generation == timer.generation {
            start_run(slot, &self.config, &mut self.runs);
        }
    }

    /// Only called after a transition to `Dead`, the sole way the ratio can rise.
    fn check_threshold(&self) -> ControlFlow<Outcome> {
        let Some(threshold) = self.config.dead_tasks_threshold else {
            return ControlFlow::Continue(());
        };
        let total = self.tasks.len();
        let dead = self
            .tasks
            .values()
            .filter(|slot| slot.status == TaskStatus::Dead)
            .count();
        if (dead as f64 / total as f64) < threshold {
            return ControlFlow::Continue(());
        }
        let err = SupervisorError::TooManyDeadTasks {
            dead,
            total,
            threshold,
        };
        error!("{err}");
        ControlFlow::Break(Err(err))
    }

    /// Cancels every run and waits for all of them to exit. Each run bounds
    /// itself with `stop_timeout`, so this returns within that delay.
    async fn stop_all(&mut self) {
        self.timers.clear();
        for slot in self.tasks.values_mut() {
            match slot.status {
                TaskStatus::Running | TaskStatus::Stopping => slot.request_stop(false),
                TaskStatus::Restarting => slot.status = TaskStatus::Dead,
                TaskStatus::Completed | TaskStatus::Dead => {}
            }
        }
        while let Some(exit) = self.runs.join_next().await {
            if let Ok((name, Err(err))) = exit {
                warn!(task = %name, error = %err, "stopped with error");
            }
        }
        info!("all tasks stopped");
    }
}

/// Spawns one run of `slot`'s task. The run yields `(name, result)`; a panic
/// inside `run` is caught and reported as `Err`, and after cancellation the
/// run is dropped once `stop_timeout` elapses.
fn start_run(slot: &mut Slot, config: &Config, runs: &mut JoinSet<RunOutput>) {
    let cancel = CancellationToken::new();
    let mut run = slot.task.clone_box().run(cancel.clone());
    let grace = config.stop_timeout;
    let name = Arc::clone(&slot.name);
    let run_cancel = cancel.clone();

    runs.spawn(async move {
        let run = async {
            tokio::select! {
                result = &mut run => result,
                _ = run_cancel.cancelled() => match timeout(grace, &mut run).await {
                    Ok(result) => result,
                    Err(_) => Err("did not stop within stop_timeout".into()),
                },
            }
        };
        tokio::pin!(run);
        let result = poll_fn(
            |cx| match catch_unwind(AssertUnwindSafe(|| run.as_mut().poll(cx))) {
                Ok(poll) => poll,
                Err(payload) => {
                    Poll::Ready(Err(format!("panicked: {}", panic_message(payload)).into()))
                }
            },
        )
        .await;
        (name, result)
    });

    slot.status = TaskStatus::Running;
    slot.cancel = Some(cancel);
    slot.generation += 1;
    slot.restart_after_stop = false;
    debug!(task = %slot.name, generation = slot.generation, "started");
}

/// Stand-in deadline for delays that overflow the clock: effectively "never".
fn far_future() -> Instant {
    Instant::now() + Duration::from_secs(30 * 365 * 24 * 3600)
}
