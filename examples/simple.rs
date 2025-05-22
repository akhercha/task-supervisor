use async_trait::async_trait;
use std::error::Error;
use std::time::Duration;
use task_supervisor::{SupervisedTask, SupervisorBuilder, TaskOutcome};

#[derive(Clone)]
struct MyTask {
    pub emoji: char,
}

// A simple task that runs for 15 seconds, printing its status periodically.
#[async_trait]
impl SupervisedTask for MyTask {
    async fn run(&mut self) -> Result<TaskOutcome, Box<dyn Error + Send + Sync>> {
        for _ in 0..15 {
            println!("{} Task is running!", self.emoji);
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        println!("{} Task completed!", self.emoji);
        Ok(TaskOutcome::Completed)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Build the supervisor with no initial tasks
    let supervisor = SupervisorBuilder::default().build();

    // Run the supervisor and get the handle
    let handle = supervisor.run();

    // Clone the handle for use in a separate task
    let h = handle.clone();

    // Spawn a task to manage and monitor the supervisor
    tokio::spawn(async move {
        // Add a new task after 5 seconds
        tokio::time::sleep(Duration::from_secs(5)).await;
        println!("Adding a task after 5 seconds...");
        h.add_task("task1", MyTask { emoji: '🆕' })
            .expect("Failed to add task");

        // Check the status of the task after 2 seconds
        tokio::time::sleep(Duration::from_secs(2)).await;
        match h.get_task_status("task1").await {
            Ok(Some(status)) => println!("Task 'task1' status: {:?}", status),
            Ok(None) => println!("Task 'task1' not found"),
            Err(e) => println!("Error getting task status: {}", e),
        }

        // Restart the task after 5 seconds
        tokio::time::sleep(Duration::from_secs(5)).await;
        println!("Restarting task after 5 seconds...");
        h.restart("task1").expect("Failed to restart task");

        // Check all task statuses after 2 seconds
        tokio::time::sleep(Duration::from_secs(2)).await;
        match h.get_all_task_statuses().await {
            Ok(statuses) => {
                println!("All task statuses:");
                for (name, status) in statuses {
                    println!("  {}: {:?}", name, status);
                }
            }
            Err(e) => println!("Error getting all task statuses: {}", e),
        }

        // Kill the task after 5 seconds
        tokio::time::sleep(Duration::from_secs(5)).await;
        println!("Killing task after 5 seconds...");
        h.kill_task("task1").expect("Failed to kill task");

        // Check the status again after killing
        tokio::time::sleep(Duration::from_secs(2)).await;
        match h.get_task_status("task1").await {
            Ok(Some(status)) => println!("Task 'task1' status after kill: {:?}", status),
            Ok(None) => println!("Task 'task1' not found after kill"),
            Err(e) => println!("Error getting task status after kill: {}", e),
        }

        // Shutdown the supervisor after 5 seconds
        tokio::time::sleep(Duration::from_secs(5)).await;
        println!("Shutting down supervisor...");
        h.shutdown().expect("Failed to shutdown supervisor");
    });

    // Wait for the supervisor to complete
    handle.wait().await??;
    println!("All tasks died and supervisor shut down! 🫡");

    Ok(())
}
