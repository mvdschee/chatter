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

/// The newest report's time: `t`, NULL without reports.
pub const REPORTS_LATEST_TIME: &str = "SELECT max(received_at) AS t FROM reports";

/// The newest `?1` reports: `id`, `received_at`, newest first.
pub const REPORTS_LAST_N: &str =
	"SELECT id, received_at FROM reports ORDER BY received_at DESC, id DESC LIMIT ?1";

/// Reports per bucket of `?2` seconds from `?1`: `bucket`, `n`.
pub const REPORTS_PER_BUCKET: &str = "SELECT (received_at - ?1) / ?2 AS bucket, count(*) AS n \
	FROM reports WHERE received_at >= ?1 GROUP BY bucket";

/// Each sensor's newest report: `sensor`, `channel`, `dropped`, `overflow`,
/// `last_seen` (SQLite takes the bare columns from the row with the max).
pub const REPORTS_SENSORS: &str = "SELECT sensor, channel, dropped, overflow, \
	max(received_at) AS last_seen FROM reports GROUP BY sensor";

// --- device_minutes --------------------------------------------------------

/// Every device: `mac`, `first_seen`, `last_seen`, and `rssi_max` since `?1`.
pub const DEVICE_MINUTES_DEVICES: &str = "SELECT mac, min(received_at) AS first_seen, \
	max(received_at) AS last_seen, max(CASE WHEN received_at >= ?1 THEN rssi_max END) AS rssi_max \
	FROM device_minutes GROUP BY mac";

/// Bytes per device per bucket of `?2` seconds from `?1`: `mac`, `bucket`,
/// `tx`, `rx`.
pub const DEVICE_MINUTES_PER_BUCKET: &str = "SELECT mac, (received_at - ?1) / ?2 AS bucket, \
	sum(tx_bytes) AS tx, sum(rx_bytes) AS rx FROM device_minutes WHERE received_at >= ?1 \
	GROUP BY mac, bucket";

/// The device rows of the newest `?1` reports: `report_id`, `mac`,
/// `tx_bytes`, `rx_bytes`.
pub const DEVICE_MINUTES_LAST_N_REPORTS: &str = "SELECT report_id, mac, tx_bytes, rx_bytes \
	FROM device_minutes WHERE report_id IN \
	(SELECT id FROM reports ORDER BY received_at DESC, id DESC LIMIT ?1)";

/// A child of `REPORTS_INSERT`: `?1` is the report's id.
pub const DEVICE_MINUTES_INSERT: &str = "INSERT INTO device_minutes \
	(report_id, mac, net, received_at, tx_frames, tx_bytes, rx_frames, rx_bytes, retries, rssi_max) \
	VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)";

// --- broadcast_minutes -----------------------------------------------------

/// A child of `REPORTS_INSERT`: `?1` is the report's id.
pub const BROADCAST_MINUTES_INSERT: &str = "INSERT INTO broadcast_minutes \
	(report_id, net, rx_frames, rx_bytes) VALUES (?1, ?2, ?3, ?4)";

// --- scans -----------------------------------------------------------------

pub const SCANS_INSERT: &str = "INSERT INTO scans (kind, targets, started_at, finished_at) \
	VALUES (?1, ?2, ?3, ?4) RETURNING id";

/// When the newest port scan finished: `t`, NULL before the first.
pub const SCANS_LAST_PORT_SCAN: &str =
	"SELECT max(finished_at) AS t FROM scans WHERE kind = 'ports'";

/// The port scans that finished since `?1`, oldest first: `started_at`,
/// `finished_at`, and how many `hosts` and `open_ports` they found, and how
/// many hosts `timed_out`.
pub const SCANS_PORT_SCANS_SINCE: &str = "SELECT started_at, finished_at, \
	(SELECT count(*) FROM scan_hosts h WHERE h.scan_id = s.id) AS hosts, \
	(SELECT count(*) FROM scan_hosts h WHERE h.scan_id = s.id AND h.timed_out = 1) AS timed_out, \
	(SELECT count(*) FROM scan_ports p WHERE p.scan_id = s.id) AS open_ports \
	FROM scans s WHERE kind = 'ports' AND finished_at >= ?1 ORDER BY started_at";

// --- scan_hosts ------------------------------------------------------------

/// A child of `SCANS_INSERT`: `?1` is the scan's id.
pub const SCAN_HOSTS_INSERT: &str = "INSERT INTO scan_hosts \
	(scan_id, mac, ip, hostname, vendor, timed_out) VALUES (?1, ?2, ?3, ?4, ?5, ?6)";

/// Each MAC's newest sighting: `mac`, `ip`, `hostname`, `vendor` (SQLite takes
/// the bare columns from the row with the max).
pub const SCAN_HOSTS_LATEST: &str =
	"SELECT mac, ip, hostname, vendor, max(scan_id) AS scan_id FROM scan_hosts GROUP BY mac";

// --- scan_ports ------------------------------------------------------------

/// A child of `SCANS_INSERT`: `?1` is the scan's id.
pub const SCAN_PORTS_INSERT: &str = "INSERT INTO scan_ports \
	(scan_id, mac, port, protocol, service, product) VALUES (?1, ?2, ?3, ?4, ?5, ?6)";

/// Each MAC's newest port scan that wasn't cut short: `mac`, `finished_at`.
pub const SCAN_HOSTS_LAST_PORT_SCAN: &str = "SELECT h.mac, max(s.finished_at) AS finished_at \
	FROM scan_hosts h JOIN scans s ON s.id = h.scan_id \
	WHERE s.kind = 'ports' AND h.timed_out = 0 GROUP BY h.mac";

/// The open ports from each MAC's newest port scan that wasn't cut short:
/// `mac`, `port`, `protocol`, `service`, `product`.
pub const SCAN_PORTS_LATEST: &str = "SELECT p.mac, p.port, p.protocol, p.service, p.product \
	FROM scan_ports p JOIN \
	(SELECT h.mac, max(h.scan_id) AS scan_id FROM scan_hosts h JOIN scans s ON s.id = h.scan_id \
	WHERE s.kind = 'ports' AND h.timed_out = 0 GROUP BY h.mac) l \
	ON l.mac = p.mac AND l.scan_id = p.scan_id ORDER BY p.mac, p.protocol, p.port";
