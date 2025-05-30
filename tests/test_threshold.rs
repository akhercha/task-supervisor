mod common;

use std::{sync::Arc, time::Duration};
use task_supervisor::{SupervisorBuilder, SupervisorError, TaskStatus}; // Ensure SupervisorError is pub
use tokio::time::pause;

use common::{HealthyTask, ImmediateFailTask};

#[tokio::test]
async fn test_supervisor_shuts_down_when_dead_task_threshold_exceeded() {
    pause();

    let supervisor = SupervisorBuilder::new()
        .with_health_check_interval(Duration::from_millis(10)) // Frequent checks
        .with_max_restart_attempts(1) // Task becomes Dead quickly
        .with_base_restart_delay(Duration::from_millis(10)) // Quick restart
        .with_dead_tasks_threshold(Some(0.40)) // Shutdown if >40% tasks are Dead
        .build();

    let handle = supervisor.run();

    let healthy_run_flag = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let healthy_task = HealthyTask {
        run_flag: healthy_run_flag.clone(),
    };
    handle.add_task("healthy_task", healthy_task).unwrap();

    // Task that will fail and become 'Dead'
    let failing_task = ImmediateFailTask;
    handle.add_task("failing_task", failing_task).unwrap();

    // --- Let time pass for tasks to start and fail ---
    // Initial run of failing_task:
    // - Starts, fails immediately.
    // - Marked 'Failed', restart_attempts = 1.
    // - Scheduled for restart after 10ms.
    tokio::time::sleep(Duration::from_millis(5)).await; // Let tasks spawn and fail_task complete its first run

    // At this point, failing_task should be 'Failed' and pending restart. healthy_task 'Healthy'.
    // Supervisor check: 0 dead / 2 total = 0% dead. Threshold not met.

    // Restart of failing_task:
    // - Restarts after 10ms delay.
    // - Fails immediately again.
    // - Since max_restart_attempts is 1, and it has now attempted 1 restart, it becomes 'Dead'.
    tokio::time::sleep(Duration::from_millis(15)).await; // 5ms past the 10ms restart delay

    // Supervisor check after failing_task becomes Dead:
    // - Tasks: healthy_task (Healthy), failing_task (Dead)
    // - Dead tasks: 1 out of 2 total tasks = 50%
    // - 50% > 40% (threshold), so supervisor should initiate shutdown.
    // - Supervisor should clean up healthy_task and terminate.

    // Wait for the supervisor to shut down.
    // The `wait()` method should now return the specific error.
    let result = handle.wait().await;

    // Assert that the supervisor shut down due to the dead task threshold.
    match result {
        Err(SupervisorError::TooManyDeadTasks {
            current_percentage,
            threshold,
        }) => {
            // current_percentage reported by the error is raw value * 100
            // threshold reported by the error is raw value * 100
            assert!(
                (current_percentage - 0.5).abs() < 0.01,
                "Expected current percentage to be around 50.0, got {}",
                current_percentage
            );
            assert!(
                (threshold - 0.4).abs() < 0.01,
                "Expected threshold to be around 40.0, got {}",
                threshold
            );
        }
        Ok(()) => {
            panic!("Supervisor shut down normally, but expected TooManyDeadTasks error.");
        }
    }
}

#[tokio::test]
async fn test_supervisor_does_not_shut_down_if_threshold_not_met() {
    pause();

    let supervisor = SupervisorBuilder::new()
        .with_health_check_interval(Duration::from_millis(10))
        .with_max_restart_attempts(1)
        .with_base_restart_delay(Duration::from_millis(10))
        .with_dead_tasks_threshold(Some(0.60)) // Shutdown if >60% tasks are Dead
        .build();

    let handle = supervisor.run();

    let healthy_run_flag = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let healthy_task = HealthyTask {
        run_flag: healthy_run_flag.clone(),
    };
    handle.add_task("healthy_task", healthy_task).unwrap();

    let failing_task = ImmediateFailTask;
    handle.add_task("failing_task", failing_task).unwrap(); // 1/2 tasks = 50% dead

    // tokio::time::sleep time for failing_task to become Dead
    tokio::time::sleep(Duration::from_millis(30)).await; // Enough time for failure and restart+failure

    // At this point, 1/2 tasks (50%) are dead. Threshold is >60%. Supervisor should NOT shut down.
    // We can check the status of the healthy task.
    let healthy_status = handle.get_task_status("healthy_task").await;
    assert_eq!(
        healthy_status.unwrap().unwrap(),
        TaskStatus::Healthy,
        "Healthy task should still be healthy"
    );

    let failing_status = handle.get_task_status("failing_task").await;
    assert_eq!(
        failing_status.unwrap().unwrap(),
        TaskStatus::Dead,
        "Failing task should be dead"
    );

    // Shut down the healthy task and then the supervisor gracefully.
    healthy_run_flag.store(false, std::sync::atomic::Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(60)).await; // Let healthy task complete
    let healthy_status_completed = handle.get_task_status("healthy_task").await;
    assert_eq!(
        healthy_status_completed.unwrap().unwrap(),
        TaskStatus::Completed,
        "Healthy task should complete"
    );

    handle.shutdown().unwrap();
    let result = handle.wait().await;
    assert!(
        matches!(result, Ok(())),
        "Supervisor should shut down normally"
    );
}
