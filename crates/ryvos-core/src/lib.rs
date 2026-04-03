//! Core types, traits, and configuration for the Ryvos agent runtime.
//!
//! This crate is the foundation that every other crate depends on. It defines:
//!
//! - **Traits**: [`LlmClient`], [`Tool`], [`ChannelAdapter`], [`SessionStore`]
//!   are the four extension points that make Ryvos pluggable.
//! - **Types**: [`ChatMessage`], [`StreamDelta`], [`ToolResult`], [`ToolContext`],
//!   [`AgentEvent`], and the full conversation model.
//! - **Config**: [`AppConfig`] and all nested configuration structs, parsed from
//!   TOML with `${ENV_VAR}` expansion.
//! - **Events**: [`EventBus`] for pub/sub communication between components.
//! - **Goals**: Weighted success criteria with deterministic and LLM-based evaluation.
//! - **Security**: Deprecated tier-based security (kept for compat), plus
//!   `tool_has_side_effects()` and `summarize_input()` used by the safety pipeline.

pub mod config;
pub mod error;
pub mod event;
pub mod goal;
pub mod hooks;
pub mod security;
pub mod traits;
pub mod types;

pub use config::AppConfig;
pub use config::IntegrationsConfig;
pub use error::{Result, RyvosError};
pub use event::EventBus;
pub use types::*;
