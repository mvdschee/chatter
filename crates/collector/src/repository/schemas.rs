//! Every SQL query the collector runs.

// --- reports ---------------------------------------------------------------

pub const REPORTS_INSERT: &str = "INSERT INTO reports \
	(sensor, seq, uptime_s, received_at, channel, dropped, overflow, nets_known, boot) \
	VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) RETURNING id";

/// The same report stored before (a resend), from the same boot: `id`.
pub const REPORTS_FIND_IN_BOOT: &str =
	"SELECT id FROM reports WHERE sensor = ?1 AND boot = ?2 AND seq = ?3";

/// For firmware without a boot id: the same report stored before (a
/// resend), received from `?4` up to `?5`: `id`.
pub const REPORTS_FIND: &str = "SELECT id FROM reports WHERE sensor = ?1 AND seq = ?2 \
	AND uptime_s = ?3 AND received_at BETWEEN ?4 AND ?5";

/// The sensor's latest report: `seq`.
pub const REPORTS_LAST_SEQ: &str =
	"SELECT seq FROM reports WHERE sensor = ?1 ORDER BY received_at DESC, id DESC LIMIT 1";

/// The newest report's time: `t`, NULL without reports.
pub const REPORTS_LATEST_TIME: &str = "SELECT max(received_at) AS t FROM reports";

/// The newest `?1` reports: `id`, `received_at`, newest first.
pub const REPORTS_LAST_N: &str =
	"SELECT id, received_at FROM reports ORDER BY received_at DESC, id DESC LIMIT ?1";

/// Each sensor's newest report: `sensor`, `id`, `seq`, `uptime_s`, `channel`,
/// `dropped`, `overflow`, `nets_known`, `last_seen` (SQLite takes the bare
/// columns from the row with the max).
pub const REPORTS_SENSORS: &str = "SELECT sensor, id, seq, uptime_s, channel, dropped, overflow, \
	nets_known, max(received_at) AS last_seen FROM reports GROUP BY sensor";

/// Every sensor's reports since `?1`, in order: `sensor`, `seq`,
/// `received_at`.
pub const REPORTS_TIMES_SINCE: &str = "SELECT sensor, seq, received_at FROM reports \
	WHERE received_at >= ?1 ORDER BY sensor, received_at, id";

/// Every report from `?1` up to `?2` with the bytes of all its devices:
/// `received_at`, `total`.
pub const REPORTS_TOTALS_BETWEEN: &str = "SELECT r.received_at, coalesce((SELECT \
	sum(d.tx_bytes + d.rx_bytes) FROM device_minutes d WHERE d.report_id = r.id), 0) AS total \
	FROM reports r WHERE r.received_at >= ?1 AND r.received_at < ?2";

/// Every report from `?1` up to `?2`: `id`, `received_at`.
pub const REPORTS_BETWEEN: &str =
	"SELECT id, received_at FROM reports WHERE received_at >= ?1 AND received_at < ?2";

// --- device_minutes --------------------------------------------------------

/// How many devices report `?1` has: `n`.
pub const DEVICE_MINUTES_COUNT_FOR_REPORT: &str =
	"SELECT count(*) AS n FROM device_minutes WHERE report_id = ?1";

/// Every device: `mac`, `first_seen`, `last_seen`, and `rssi_max` since `?1`.
pub const DEVICE_MINUTES_DEVICES: &str = "SELECT mac, min(received_at) AS first_seen, \
	max(received_at) AS last_seen, max(CASE WHEN received_at >= ?1 THEN rssi_max END) AS rssi_max \
	FROM device_minutes GROUP BY mac";

/// Bytes per device per bucket of `?2` seconds from `?1` up to `?3`: `mac`,
/// `bucket`, `tx`, `rx`.
pub const DEVICE_MINUTES_PER_BUCKET_BETWEEN: &str = "SELECT mac, (received_at - ?1) / ?2 AS bucket, \
	sum(tx_bytes) AS tx, sum(rx_bytes) AS rx FROM device_minutes \
	WHERE received_at >= ?1 AND received_at < ?3 GROUP BY mac, bucket";

/// The device rows of the newest `?1` reports: `report_id`, `mac`,
/// `tx_bytes`, `rx_bytes`.
pub const DEVICE_MINUTES_LAST_N_REPORTS: &str = "SELECT report_id, mac, tx_bytes, rx_bytes \
	FROM device_minutes WHERE report_id IN \
	(SELECT id FROM reports ORDER BY received_at DESC, id DESC LIMIT ?1)";

/// Each device's bytes per report from `?1` up to `?2`: `report_id`, `mac`,
/// `bytes` (sent and received).
pub const DEVICE_MINUTES_BYTES_BETWEEN: &str = "SELECT report_id, mac, \
	sum(tx_bytes + rx_bytes) AS bytes FROM device_minutes \
	WHERE received_at >= ?1 AND received_at < ?2 GROUP BY report_id, mac";

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

/// How many port scans there were before scan `?1`: `n`. None: it's the
/// baseline.
pub const SCANS_PORT_SCANS_BEFORE: &str =
	"SELECT count(*) AS n FROM scans WHERE kind = 'ports' AND id < ?1";

/// The port scans that overlap `?1` up to `?2`, oldest first: `started_at`,
/// `finished_at`, how many `hosts` and `open_ports` they found, how many
/// hosts `timed_out`, and how many changes were `alerts`.
pub const SCANS_PORT_SCANS_BETWEEN: &str = "SELECT started_at, finished_at, \
	(SELECT count(*) FROM scan_hosts h WHERE h.scan_id = s.id) AS hosts, \
	(SELECT count(*) FROM scan_hosts h WHERE h.scan_id = s.id AND h.timed_out = 1) AS timed_out, \
	(SELECT count(*) FROM scan_changes c WHERE c.scan_id = s.id AND c.alert = 1) AS alerts, \
	(SELECT count(*) FROM scan_ports p WHERE p.scan_id = s.id) AS open_ports \
	FROM scans s WHERE kind = 'ports' AND finished_at >= ?1 AND started_at < ?2 \
	ORDER BY started_at";

/// When the newest port scan finished: `t`, NULL before the first.
pub const SCANS_LAST_PORT_SCAN: &str =
	"SELECT max(finished_at) AS t FROM scans WHERE kind = 'ports'";

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

/// The port scan before scan `?2` that saw MAC `?1` and wasn't cut short:
/// `scan_id`, NULL if none did.
pub const SCAN_HOSTS_PREVIOUS_PORT_SCAN: &str = "SELECT max(h.scan_id) AS scan_id \
	FROM scan_hosts h JOIN scans s ON s.id = h.scan_id \
	WHERE s.kind = 'ports' AND h.timed_out = 0 AND h.mac = ?1 AND h.scan_id < ?2";

/// The open ports scan `?1` found on MAC `?2`: `port`, `protocol`, `service`,
/// `product`.
pub const SCAN_PORTS_OF_HOST: &str =
	"SELECT port, protocol, service, product FROM scan_ports WHERE scan_id = ?1 AND mac = ?2";

// --- scan_changes ----------------------------------------------------------

pub const SCAN_CHANGES_INSERT: &str = "INSERT INTO scan_changes \
	(scan_id, mac, kind, alert, port, protocol, service, product, before) \
	VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) RETURNING id";

/// One of MAC `?1`'s ports from `?2` up has closed before: a row, else none.
pub const SCAN_CHANGES_ROTATES: &str = "SELECT 1 AS x FROM scan_changes \
	WHERE mac = ?1 AND kind = 'closed_port' AND port >= ?2 LIMIT 1";

/// The changes found by port scans that finished from `?1` up to `?2`, in
/// order: the change, when its scan finished, the host as that scan saw it,
/// and how many open ports it had then.
pub const SCAN_CHANGES_BETWEEN: &str = "SELECT c.mac, c.kind, c.alert, c.port, c.protocol, \
	c.service, c.product, c.before, s.finished_at, h.ip, h.hostname, h.vendor, \
	(SELECT count(*) FROM scan_ports p WHERE p.scan_id = c.scan_id AND p.mac = c.mac) AS ports \
	FROM scan_changes c JOIN scans s ON s.id = c.scan_id \
	LEFT JOIN scan_hosts h ON h.scan_id = c.scan_id AND h.mac = c.mac \
	WHERE s.finished_at >= ?1 AND s.finished_at < ?2 ORDER BY s.finished_at, c.id";

// --- baselines -------------------------------------------------------------

/// Every device's baselines: `mac`, `hour` (0-23, and 24 for all hours),
/// `median`, `mad`, `n`.
pub const BASELINES_ALL: &str = "SELECT mac, hour, median, mad, n FROM baselines";

pub const BASELINES_DELETE_ALL: &str = "DELETE FROM baselines";

pub const BASELINES_INSERT: &str = "INSERT INTO baselines \
	(mac, hour, median, mad, n, computed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)";

// --- floorplans ------------------------------------------------------------

pub const FLOORPLANS_INSERT: &str =
	"INSERT INTO floorplans (plan, saved_at) VALUES (?1, ?2) RETURNING id";

/// The newest plan: `plan`, as JSON.
pub const FLOORPLANS_LATEST: &str = "SELECT plan FROM floorplans ORDER BY id DESC LIMIT 1";

// --- device_names ----------------------------------------------------------

/// Every name given: `mac`, `name`.
pub const DEVICE_NAMES_ALL: &str = "SELECT mac, name FROM device_names";

pub const DEVICE_NAMES_UPSERT: &str = "INSERT INTO device_names (mac, name, named_at) \
	VALUES (?1, ?2, ?3) ON CONFLICT (mac) DO UPDATE SET name = ?2, named_at = ?3";

pub const DEVICE_NAMES_DELETE: &str = "DELETE FROM device_names WHERE mac = ?1";

// --- identify sessions -----------------------------------------------------

/// One row per identify session: started, or its end moved (extended or
/// stopped early).
pub const IDENTIFY_SESSIONS_UPSERT: &str = "INSERT INTO identify_sessions \
	(sensor, started_at, ended_at) VALUES (?1, ?2, ?3) \
	ON CONFLICT (sensor, started_at) DO UPDATE SET ended_at = ?3";

/// The identify sessions that overlap `?1` up to `?2`, oldest first.
pub const IDENTIFY_SESSIONS_BETWEEN: &str = "SELECT sensor, started_at, ended_at \
	FROM identify_sessions WHERE ended_at >= ?1 AND started_at < ?2 ORDER BY started_at";
