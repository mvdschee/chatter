//! Every SQL query the collector runs.

// --- reports ---------------------------------------------------------------

pub const REPORTS_INSERT: &str = "INSERT INTO reports \
	(sensor, seq, uptime_s, received_at, channel, dropped, overflow, nets_known) \
	VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) RETURNING id";

/// The same report stored before (a resend): `id`.
pub const REPORTS_FIND: &str =
	"SELECT id FROM reports WHERE sensor = ?1 AND seq = ?2 AND uptime_s = ?3";

/// The sensor's latest report: `seq`.
pub const REPORTS_LAST_SEQ: &str =
	"SELECT seq FROM reports WHERE sensor = ?1 ORDER BY received_at DESC, id DESC LIMIT 1";

// --- device_minutes --------------------------------------------------------

/// A child of `REPORTS_INSERT`: `?1` is the report's id.
pub const DEVICE_MINUTES_INSERT: &str = "INSERT INTO device_minutes \
	(report_id, mac, net, received_at, tx_frames, tx_bytes, rx_frames, rx_bytes, retries, rssi_max) \
	VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)";

// --- broadcast_minutes -----------------------------------------------------

/// A child of `REPORTS_INSERT`: `?1` is the report's id.
pub const BROADCAST_MINUTES_INSERT: &str = "INSERT INTO broadcast_minutes \
	(report_id, net, rx_frames, rx_bytes) VALUES (?1, ?2, ?3, ?4)";
