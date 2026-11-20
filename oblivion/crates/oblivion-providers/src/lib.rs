//! Model access for Oblivion: one OpenAI-compatible streaming client and a
//! router that falls back across free providers.

pub mod client;
pub mod router;
pub mod types;

pub use client::{Client, ProviderConfig, ProviderError, StreamEvent};
pub use router::{AllFailed, Router, Target, TargetStatus, UsageLedger};
pub use types::{
    estimate_tokens, Completion, FunctionCall, ImageAttachment, Message, Role, ToolCall, ToolSpec,
    Usage,
};
