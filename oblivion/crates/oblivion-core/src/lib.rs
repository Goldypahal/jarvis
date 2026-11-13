//! Oblivion's agent core: configuration, system prompt, sessions and the loop.

pub mod agent;
pub mod config;
pub mod prompt;
pub mod session;

pub use agent::{Agent, AgentEvent, Approver, Decision, TurnSummary};
pub use config::{ApprovalMode, Config};
pub use session::Session;
