// Set at build time from `.env` (see build.rs). Never log these.
pub const WIFI_SSID: &str = env!("CHATTER_WIFI_SSID");
pub const WIFI_PASSWORD: &str = env!("CHATTER_WIFI_PASSWORD");
/// Comma-separated; a network's position in this list is its `net` index.
pub const WATCH_SSIDS: &str = env!("CHATTER_WATCH_SSIDS");
/// `<ip>:<port>` of the collector; reports go to `POST /report`.
pub const COLLECTOR_ADDR: &str = env!("CHATTER_COLLECTOR_ADDR");

/// Names this sensor in its reports: `CHATTER_SENSOR_NAME`, or `c6-1`.
pub const SENSOR_NAME: &str = env!("CHATTER_SENSOR_NAME");
/// Where a boot asks the collector what to do.
pub const MODE_PATH: &str = concat!("/mode/", env!("CHATTER_SENSOR_NAME"));

pub const RECONNECT_DELAY_SECS: u64 = 5;
/// In 0.25 dBm steps: 60 is 15 dBm. esp-radio defaults to 20 (5 dBm), too weak
/// for the router to hear the board's full-size frames reliably. Its docs give
/// [8, 84] as the range and report authentication failures above about 65.
pub const MAX_TX_POWER: i8 = 60;
/// How often the device table is reported and emptied.
pub const PERIOD_SECS: u64 = 60;
/// How often identify mode posts the devices it heard.
pub const IDENTIFY_SECS: u64 = 3;
