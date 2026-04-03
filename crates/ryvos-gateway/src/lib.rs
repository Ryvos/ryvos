//! HTTP/WebSocket gateway for the Ryvos Web UI and REST API.
//!
//! Built on Axum, this crate provides:
//!
//! - **38 REST endpoints** for sessions, runs, costs, audit, config,
//!   approvals, cron, budget, model, integrations, goals, and webhooks.
//! - **WebSocket** server with real-time event streaming and 5 RPC methods
//!   (agent.send, agent.cancel, session.list, session.history, approval.respond).
//! - **Authentication** with API key roles (Viewer, Operator, Admin) and
//!   anonymous Admin mode for self-hosted single-user deployments.
//! - **OAuth 2.0** flow for Gmail, Slack, GitHub, Jira, and Linear.
//! - **Embedded Web UI** served via `rust_embed` (Svelte 5 SPA, ~376KB).

mod auth;
mod connection;
mod lane;
mod middleware;
pub mod oauth;
mod protocol;
mod routes;
mod server;
mod state;
mod static_files;

pub use ryvos_core::IntegrationsConfig;
pub use server::GatewayServer;
