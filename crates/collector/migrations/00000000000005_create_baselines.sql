-- Each device's usual traffic per hour of the day, over its minutes in the
-- last 14 days: the median and the MAD of ln(1 + bytes a minute). Worked out
-- again every hour, see docs/outliers.md.

CREATE TABLE baselines (
	mac TEXT NOT NULL,
	-- 0-23, in the collector's time zone
	hour INTEGER NOT NULL,
	median REAL NOT NULL,
	mad REAL NOT NULL,
	-- the device's minutes in this hour; under 60, median and mad are its
	-- baseline over all hours instead
	n INTEGER NOT NULL,
	computed_at INTEGER NOT NULL,
	PRIMARY KEY (mac, hour)
);
