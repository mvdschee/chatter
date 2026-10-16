// Set at build time from `.env` (see build.rs). Never log these.
pub const WIFI_SSID: &str = env!("CHATTER_WIFI_SSID");
pub const WIFI_PASSWORD: &str = env!("CHATTER_WIFI_PASSWORD");
/// Comma-separated; a network's position in this list is its `net` index.
pub const WATCH_SSIDS: &str = env!("CHATTER_WATCH_SSIDS");
/// `<ip>:<port>` of the collector; reports go to `POST /report`.
pub const COLLECTOR_ADDR: &str = env!("CHATTER_COLLECTOR_ADDR");

/// Names this sensor in its reports. Plain ASCII, no quotes.
pub const SENSOR_NAME: &str = "c6-1";

pub const RECONNECT_DELAY_SECS: u64 = 5;
/// How often the device table is reported and emptied.
pub const PERIOD_SECS: u64 = 60;
