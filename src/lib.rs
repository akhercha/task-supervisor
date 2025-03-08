//! # Supervisor for Tokio async tasks Rust.
pub use supervisor::{Supervisor, SupervisorBuilder};
pub use task::SupervisedTask;

mod supervisor;
mod task;
