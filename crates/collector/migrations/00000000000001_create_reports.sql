-- One row per report a sensor posts (every 60 s), and its device and
-- broadcast counters. Times are unix seconds, when the collector got the report.

CREATE TABLE reports (
	id INTEGER PRIMARY KEY,
	sensor TEXT NOT NULL,
	seq INTEGER NOT NULL,
	uptime_s INTEGER NOT NULL,
	received_at INTEGER NOT NULL,
	channel INTEGER NOT NULL,
	dropped INTEGER NOT NULL,
	overflow INTEGER NOT NULL,
	-- JSON array of the watched networks' indexes the sensor knows a BSSID for
	nets_known TEXT NOT NULL,
	-- a resent report (its answer got lost) has the same seq and uptime
	UNIQUE (sensor, seq, uptime_s)
);

CREATE INDEX reports_sensor_time ON reports (sensor, received_at);

CREATE TABLE device_minutes (
	report_id INTEGER NOT NULL REFERENCES reports (id) ON DELETE CASCADE,
	mac TEXT NOT NULL,
	net INTEGER NOT NULL,
	-- copied from the report, so per-device queries over time need no join
	received_at INTEGER NOT NULL,
	tx_frames INTEGER NOT NULL,
	tx_bytes INTEGER NOT NULL,
	rx_frames INTEGER NOT NULL,
	rx_bytes INTEGER NOT NULL,
	retries INTEGER NOT NULL,
	-- NULL when the device sent nothing the sensor could hear
	rssi_max INTEGER
);

CREATE INDEX device_minutes_mac_time ON device_minutes (mac, received_at);
CREATE INDEX device_minutes_report ON device_minutes (report_id);

CREATE TABLE broadcast_minutes (
	report_id INTEGER NOT NULL REFERENCES reports (id) ON DELETE CASCADE,
	net INTEGER NOT NULL,
	rx_frames INTEGER NOT NULL,
	rx_bytes INTEGER NOT NULL
);

CREATE INDEX broadcast_minutes_report ON broadcast_minutes (report_id);
