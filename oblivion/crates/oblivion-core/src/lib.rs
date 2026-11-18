//! Oblivion's agent core: configuration, system prompt, sessions and the loop.

pub mod agent;
pub mod compact;
pub mod config;
pub mod extract;
pub mod prompt;
pub mod rules;
pub mod session;
pub mod subagent;

pub use agent::{Agent, AgentEvent, Approver, Decision, TurnSummary};
pub use config::{ApprovalMode, Config};
pub use extract::{
    extract_heuristics, extract_memories, parse_proposals, MemoryRecall, ProposedMemory,
    RecallFuture,
};
pub use rules::Rules;
pub use session::{JsonDir, Session, SessionStore};
pub use subagent::SubagentFactory;
