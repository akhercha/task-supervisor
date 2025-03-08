use anyhow::bail;
use async_trait::async_trait;
use std::time::Duration;
use task_supervisor::{SupervisedTask, SupervisorBuilder};

#[derive(Clone, Default)]
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
    type Error = anyhow::Error;

    async fn run_forever(&mut self) -> anyhow::Result<()> {
        let mut i = 0;
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            println!("{} Task is running!", self.emoji);
            i += 1;
            if i == 5 {
                println!("{} Task is failing after 5 iterations...", self.emoji);
                bail!("Task failed after 5 iterations");
            }
        }
    }
}

#[tokio::main]
async fn main() {
    // Build the supervisor with initial tasks
    let supervisor = SupervisorBuilder::default()
        .with_task(MyTask::new('🥴'))
        .with_task(MyTask::new('🧑'))
        .with_task(MyTask::new('😸'))
        .with_task(MyTask::new('👽'))
        .build();

    // Run the supervisor and get the handle
    let handle = supervisor.run();

    // Spawn a task to add a new task after 5 seconds
    tokio::time::sleep(Duration::from_secs(5)).await;
    println!("Adding a new task after 5 seconds...");
    handle.add_task(MyTask::new('🆕'));

    // Wait for all tasks to die
    handle.wait().await;
    println!("All tasks died! 🫡");
}
