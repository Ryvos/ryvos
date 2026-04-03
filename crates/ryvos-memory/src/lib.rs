//! Persistence layer for Ryvos: sessions, memory, costs, and integrations.
//!
//! All storage is SQLite-based with no external database dependencies.
//!
//! - **VikingStore**: Hierarchical memory with L0/L1/L2 detail levels,
//!   FTS5 full-text search, and the `viking://` URI protocol.
//! - **SqliteStore**: Session message persistence with optional embeddings.
//! - **CostStore**: Per-run cost tracking with monthly spend aggregation.
//! - **SessionMetaStore**: Session metadata (channel, billing, token counts).
//! - **IntegrationStore**: OAuth token storage for external services.
//! - **VikingClient**: HTTP client for the standalone Viking server.
//! - **Pricing**: Model pricing estimation for cost calculations.

pub mod cost_store;
pub mod embeddings;
pub mod integration_store;
pub mod pricing;
pub mod session_meta;
pub mod store;
pub mod viking;
pub mod viking_store;

pub use cost_store::CostStore;
pub use integration_store::{IntegrationStore, IntegrationToken};
pub use pricing::estimate_cost_cents;
pub use session_meta::SessionMetaStore;
pub use store::SqliteStore;
pub use viking::VikingClient;
pub use viking_store::VikingStore;
