use async_trait::async_trait;
use std::time::Duration;
use task_supervisor::{SupervisedTask, SupervisorBuilder, TaskOutcome};

#[derive(Clone)]
struct MyTask {
    pub emoji: char,
}

impl MyTask {
    fn new(emoji: char) -> Self {
        Self { emoji }
    }
}

#[async_trait]
impl SupervisedTask for MyTask {
    async fn run(&mut self) -> Result<TaskOutcome, Box<dyn std::error::Error + Send + Sync>> {
        let mut i = 0;
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            println!("{} Task is running!", self.emoji);
            i += 1;
            if i == 5 {
                println!("{} Task is failing after 5 iterations...", self.emoji);
                return Err("Task failed after 5 iterations".into());
            }
        }
    }

    fn clone_task(&self) -> Box<dyn SupervisedTask> {
        Box::new(self.clone())
    }
}

#[tokio::main]
async fn main() {
    // Build the supervisor with initial tasks
    let supervisor = SupervisorBuilder::default()
        .with_task("task1".into(), MyTask::new('🥴'))
        .with_task("task2".into(), MyTask::new('🧑'))
        .build();

    // Run the supervisor and get the handle
    let handle = supervisor.run();

    // Spawn a task to add a new task after 5 seconds
    let h = handle.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        println!("Adding a new task after 5 seconds...");
        let _ = h.add_task("task3".into(), MyTask::new('🆕'));
    });

    // Wait for all tasks to die
    let _ = handle.wait().await;
    println!("All tasks died! 🫡");
}
