-- no-transaction
-- A reboot repeats a report's seq and uptime, so (sensor, seq, uptime_s)
-- can't be unique: a resend is only one whose copy came in within the hour
-- around it (`REPORTS_FIND`). See docs/identify.md, Pitfalls.
--
-- SQLite can't drop a constraint, so the table is rebuilt. Foreign keys are
-- off for it, or dropping the old table would delete every device and
-- broadcast minute with it; that pragma only works outside a transaction.

PRAGMA foreign_keys = OFF;

BEGIN;

CREATE TABLE reports_new (
	id INTEGER PRIMARY KEY,
	sensor TEXT NOT NULL,
	seq INTEGER NOT NULL,
	uptime_s INTEGER NOT NULL,
	received_at INTEGER NOT NULL,
	channel INTEGER NOT NULL,
	dropped INTEGER NOT NULL,
	overflow INTEGER NOT NULL,
	-- JSON array of the watched networks' indexes the sensor knows a BSSID for
	nets_known TEXT NOT NULL
);

INSERT INTO reports_new
	(id, sensor, seq, uptime_s, received_at, channel, dropped, overflow, nets_known)
	SELECT id, sensor, seq, uptime_s, received_at, channel, dropped, overflow, nets_known
	FROM reports;

DROP TABLE reports;

ALTER TABLE reports_new RENAME TO reports;

CREATE INDEX reports_sensor_time ON reports (sensor, received_at);

COMMIT;

PRAGMA foreign_keys = ON;
