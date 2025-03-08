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

#[async_trait::async_trait]
impl SupervisedTask for MyTask {
    // Using anyhow for simplicity but could be your Error type
    type Error = anyhow::Error;

    async fn run_forever(&mut self) -> anyhow::Result<()> {
        let mut i = 0;
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            println!("{} Task is running!", self.emoji);
            i += 1;
            if i == 5 {
                break;
            }
        }
        println!("End of the task...");
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    let mut supervisor = SupervisorBuilder::default()
        .with_task(MyTask::new('🥴'))
        .with_task(MyTask::new('🧑'))
        .with_task(MyTask::new('😸'))
        .with_task(MyTask::new('👽'))
        .build();

    supervisor.run_and_supervise().await;
    println!("All tasks died! The end 🫡");
}
