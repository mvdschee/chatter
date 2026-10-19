-- What changed per device since its port scan before (the infra repo's
-- lan_scan rules): one row per new device, new, closed or changed port.

CREATE TABLE scan_changes (
	id INTEGER PRIMARY KEY,
	scan_id INTEGER NOT NULL REFERENCES scans (id) ON DELETE CASCADE,
	mac TEXT NOT NULL,
	-- 'new_device', 'new_port', 'new_rotating_port', 'closed_port', 'changed_port'
	kind TEXT NOT NULL,
	-- 1: worth an alert (a new device, a new port); 0: for information
	alert INTEGER NOT NULL,
	-- the port, for every kind but 'new_device'
	port INTEGER,
	protocol TEXT,
	service TEXT,
	product TEXT,
	-- 'changed_port': the product before
	before TEXT
);

CREATE INDEX scan_changes_scan ON scan_changes (scan_id);
CREATE INDEX scan_changes_mac ON scan_changes (mac, kind);
