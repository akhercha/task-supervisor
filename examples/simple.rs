use std::time::Duration;

use anyhow::Result;
use task_supervisor::{SupervisedTask, SupervisorBuilder, TaskOutcome};

/// Task Type 1: A counter that completes after N iterations
#[derive(Clone)]
struct CounterTask {
    name: String,
    max_count: u32,
    current: u32,
}

impl CounterTask {
    fn new(name: &str, max_count: u32) -> Self {
        Self {
            name: name.to_string(),
            max_count,
            current: 0,
        }
    }
}

// Task Type 2: A clock that runs indefinitely
#[derive(Clone)]
struct ClockTask {
    name: String,
    interval_ms: u64,
}

impl ClockTask {
    fn new(name: &str, interval_ms: u64) -> Self {
        Self {
            name: name.to_string(),
            interval_ms,
        }
    }
}

// SupervisedTask implementation for CounterTask
#[async_trait::async_trait]
impl SupervisedTask for CounterTask {
    async fn run(&mut self) -> Result<TaskOutcome, Box<dyn std::error::Error + Send + Sync>> {
        println!("🔢 Starting counter task: {}", self.name);

        while self.current < self.max_count {
            tokio::time::sleep(Duration::from_millis(500)).await;
            self.current += 1;
            println!(
                "🔢 {}: Count {}/{}",
                self.name, self.current, self.max_count
            );
        }

        println!("✅ {}: Counter task completed!", self.name);
        Ok(TaskOutcome::Completed) // Task completes successfully
    }

    fn clone_task(&self) -> Box<dyn SupervisedTask> {
        Box::new(self.clone())
    }
}

// SupervisedTask implementation for ClockTask
#[async_trait::async_trait]
impl SupervisedTask for ClockTask {
    async fn run(&mut self) -> Result<TaskOutcome, Box<dyn std::error::Error + Send + Sync>> {
        println!("⏰ Starting clock task: {}", self.name);

        let mut ticks = 0;
        loop {
            tokio::time::sleep(Duration::from_millis(self.interval_ms)).await;
            ticks += 1;
            println!("⏰ {}: Tick #{}", self.name, ticks);

            // Occasionally fail to demonstrate restart behavior
            if ticks % 10 == 0 {
                println!("❌ {}: Clock task failing on tick #{}", self.name, ticks);
                return Err("Simulated clock failure".into());
            }
        }
    }

    fn clone_task(&self) -> Box<dyn SupervisedTask> {
        Box::new(self.clone())
    }
}

#[tokio::main]
async fn main() {
    // Create supervisor with different task types
    let supervisor = SupervisorBuilder::default()
        // Add CounterTask instances
        .with_task("counter-5".into(), CounterTask::new("Counter-5", 5))
        .with_task("counter-8".into(), CounterTask::new("Counter-8", 8))
        // Add ClockTask instances
        .with_task("clock-fast".into(), ClockTask::new("Fast-Clock", 300))
        .with_task("clock-slow".into(), ClockTask::new("Slow-Clock", 1000))
        .build();

    // Run the supervisor
    let handle = supervisor.run();

    // Demonstrate adding a different task type after startup
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(3)).await;
        println!("\n➕ Adding a new counter task after 3 seconds");
        let _ = handle.add_task("late-counter".into(), CounterTask::new("Late-Counter", 3));
    });

    // Wait for all tasks to complete or die
    println!("Supervisor running. Press Ctrl+C to exit.");
    let _ = tokio::signal::ctrl_c().await;
    println!("Shutting down supervisor...");
}
