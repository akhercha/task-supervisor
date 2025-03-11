use async_trait::async_trait;
use std::time::Duration;
use task_supervisor::{SupervisedTask, SupervisorBuilder, TaskOutcome};

#[derive(Clone)]
struct MyTask {
    pub emoji: char,
}

// A simple task that completes after 10 seconds.
#[async_trait]
impl SupervisedTask for MyTask {
    async fn run(&mut self) -> Result<TaskOutcome, Box<dyn std::error::Error + Send + Sync>> {
        for _ in 0..15 {
            println!("{} Task is running!", self.emoji);
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        println!("{} Task completed!", self.emoji);
        Ok(TaskOutcome::Completed)
    }

    fn clone_task(&self) -> Box<dyn SupervisedTask> {
        Box::new(self.clone())
    }
}

#[tokio::main]
async fn main() {
    // Build the supervisor with initial tasks
    let supervisor = SupervisorBuilder::default().build();

    // Run the supervisor and get the handle
    let handle = supervisor.run();

    let h = handle.clone();
    tokio::spawn(async move {
        // Add a new task after 5 seconds
        tokio::time::sleep(Duration::from_secs(5)).await;
        println!("Adding a task after 5 seconds...");
        h.add_task("task".into(), MyTask { emoji: '🆕' }).unwrap();

        // Restart the task after 5 seconds
        tokio::time::sleep(Duration::from_secs(5)).await;
        println!("Restarting task a new task after 5 seconds...");
        h.restart("task".into()).unwrap();

        // Restart the task after 5 seconds
        tokio::time::sleep(Duration::from_secs(5)).await;
        println!("Restarting task a new task after 5 seconds...");
        h.restart("task".into()).unwrap();

        // Shutdown the task after 5 seconds
        tokio::time::sleep(Duration::from_secs(5)).await;
        println!("Killing task a new task after 5 seconds...");
        h.kill_task("task".into()).unwrap();
    });

    // Wait for all tasks to die
    let _ = handle.wait().await;
    println!("All tasks died! 🫡");
}
