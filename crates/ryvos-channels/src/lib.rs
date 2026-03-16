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
