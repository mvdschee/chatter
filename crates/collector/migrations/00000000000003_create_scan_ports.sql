-- Port scans (scans.kind 'ports'): every open port per host, and whether a
-- host timed out (then its ports weren't all checked, and it keeps what the
-- scan before found).

ALTER TABLE scan_hosts ADD COLUMN timed_out INTEGER NOT NULL DEFAULT 0;

CREATE TABLE scan_ports (
	scan_id INTEGER NOT NULL REFERENCES scans (id) ON DELETE CASCADE,
	mac TEXT NOT NULL,
	port INTEGER NOT NULL,
	-- 'tcp' or 'udp'
	protocol TEXT NOT NULL,
	-- nmap's service name, e.g. 'http'
	service TEXT,
	-- product and version, e.g. 'lighttpd 1.4.59'
	product TEXT
);

CREATE INDEX scan_ports_mac ON scan_ports (mac, scan_id);
CREATE INDEX scan_ports_scan ON scan_ports (scan_id);
