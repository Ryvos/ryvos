//! Multi-platform messaging adapters for Ryvos.
//!
//! Each adapter implements the [`ChannelAdapter`] trait and handles
//! platform-specific concerns: authentication, message formatting,
//! approval UI (buttons/inline keyboards), and message chunking.
//!
//! - **Telegram**: Long-polling via teloxide, MarkdownV2, inline keyboards.
//! - **Slack**: Socket Mode WebSocket, Block Kit UI, message threading.
//! - **Discord**: Serenity event handler, per-guild sessions.
//! - **WhatsApp**: Cloud API webhooks, interactive buttons.
//!
//! The [`ChannelDispatcher`] routes incoming messages to the agent runtime
//! and delivers responses back through the originating adapter. It also
//! handles `/approve` and `/deny` commands, lifecycle hooks, and session
//! resume for CLI-based providers.

pub mod discord;
pub mod dispatch;
pub mod pairing;
pub mod slack;
pub mod telegram;
pub mod util;
pub mod whatsapp;

pub use discord::DiscordAdapter;
pub use dispatch::ChannelDispatcher;
pub use pairing::PairingManager;
pub use slack::SlackAdapter;
pub use telegram::TelegramAdapter;
pub use whatsapp::{WhatsAppAdapter, WhatsAppWebhookHandle};
