//! Oblivion's agent core: configuration, system prompt, sessions and the loop.

pub mod agent;
pub mod agents_md;
pub mod compact;
pub mod config;
pub mod extract;
pub mod hardware;
pub mod ollama;
pub mod prompt;
pub mod routing;
pub mod rules;
pub mod session;
pub mod small_model;
pub mod subagent;

pub use agent::{Agent, AgentEvent, Approver, Decision, TurnSummary};
pub use agents_md::AgentsMdProposal;
pub use config::{ApprovalMode, Config};
pub use extract::{
    extract_heuristics, extract_memories, parse_proposals, MemoryRecall, ProposedMemory,
    RecallFuture,
};
pub use hardware::{detect_hardware, recommend_model, HardwareInfo, ModelRecommendation};
pub use ollama::{
    format_bytes, OllamaClient, OllamaModel, OllamaModelDetails, OllamaPullProgress,
    OllamaSpeedTestResult, OllamaStatus,
};
pub use routing::{classify_request, RequestClass, RoutingRules};
pub use rules::Rules;
pub use session::{JsonDir, Session, SessionInfo, SessionStore};
pub use small_model::{extract_tool_calls_from_text, is_small_model_name, repair_tool_arguments};
pub use subagent::SubagentFactory;
