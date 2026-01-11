//! Web UI module for ANODE-EVAL
//!
//! Provides a web interface for:
//! 1. Live session monitoring - view progress of running evaluations
//! 2. Results dashboard - view all evaluation results

mod server;
mod state;
mod handlers;

pub use server::start_server;
pub use state::{AppState, SessionInfo, SessionStatus};
