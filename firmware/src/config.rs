// Set at build time from `.env` (see build.rs). Never log these.
pub const WIFI_SSID: &str = env!("CHATTER_WIFI_SSID");
pub const WIFI_PASSWORD: &str = env!("CHATTER_WIFI_PASSWORD");
/// Comma-separated; a network's position in this list is its `net` index.
pub const WATCH_SSIDS: &str = env!("CHATTER_WATCH_SSIDS");

pub const RECONNECT_DELAY_SECS: u64 = 5;
