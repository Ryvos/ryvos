//! Shared test utilities, mocks, and fixtures for the Ryvos workspace.
//!
//! Provides mock implementations of core traits (`LlmClient`, `Tool`,
//! `ChannelAdapter`, `SessionStore`) and helpers for constructing test
//! configs and tool contexts. Every crate in the workspace can add this
//! as a `[dev-dependency]` to get consistent, reusable test infrastructure.

pub mod fixtures;
pub mod mock_channel;
pub mod mock_llm;
pub mod mock_store;
pub mod mock_tool;

pub use fixtures::*;
pub use mock_channel::MockChannelAdapter;
pub use mock_llm::MockLlmClient;
pub use mock_store::InMemorySessionStore;
pub use mock_tool::MockTool;
