//! Built-in tool library and registry for Ryvos.
//!
//! Provides 60+ tools organized by category: file I/O, search, git, web,
//! browser automation, memory, scheduling, data transformation, system,
//! database, and communication. All tools implement the [`Tool`] trait
//! from `ryvos-core`.
//!
//! The [`ToolRegistry`] manages tool registration, lookup, and execution
//! with timeout enforcement. External tools (MCP, skills) are registered
//! at runtime through the same registry interface.

pub mod builtin;
pub mod registry;

pub use registry::ToolRegistry;
