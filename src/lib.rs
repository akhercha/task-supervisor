#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

mod builder;
mod handle;
mod supervisor;
mod task;

pub use builder::SupervisorBuilder;
pub use handle::{SupervisorHandle, SupervisorHandleError};
pub use supervisor::SupervisorError;
pub use task::{SupervisedTask, TaskError, TaskResult, TaskStatus};
pub use tokio_util::sync::CancellationToken;
