use std::time::Duration;

use task_supervisor::{CancellationToken, SupervisedTask, SupervisorBuilder, TaskResult};

/// Prints a line every second; fails on its fifth tick to show restarts.
#[derive(Clone)]
struct Ticker {
    label: &'static str,
}

impl SupervisedTask for Ticker {
    async fn run(self, cancel: CancellationToken) -> TaskResult {
        for tick in 1.. {
            tokio::select! {
                _ = cancel.cancelled() => {
                    println!("[{}] stopping cleanly", self.label);
                    return Ok(());
                }
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    println!("[{}] tick {tick}", self.label);
                    if tick == 5 {
                        return Err("tick 5 always fails".into());
                    }
                }
            }
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let handle = SupervisorBuilder::new()
        .with_base_restart_delay(Duration::from_secs(1))
        .with_restart_limit(2, Duration::from_secs(60))
        .with_task("a", Ticker { label: "a" })
        .spawn();

    tokio::time::sleep(Duration::from_secs(3)).await;
    handle.add_task("b", Ticker { label: "b" }).await?;

    tokio::time::sleep(Duration::from_secs(4)).await;
    println!("statuses: {:?}", handle.task_statuses().await?);

    handle.restart_task("a").await?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    handle.kill_task("b").await?;
    println!("b: {}", handle.task_status("b").await?);

    handle.shutdown().await?;
    println!("supervisor stopped");
    Ok(())
}
