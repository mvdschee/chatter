-- The collector's own network scans (nmap) and the hosts they found. Times
-- are unix seconds.

CREATE TABLE scans (
	id INTEGER PRIMARY KEY,
	-- 'sweep': host discovery (nmap -sn)
	kind TEXT NOT NULL,
	targets TEXT NOT NULL,
	started_at INTEGER NOT NULL,
	finished_at INTEGER NOT NULL
);

CREATE TABLE scan_hosts (
	scan_id INTEGER NOT NULL REFERENCES scans (id) ON DELETE CASCADE,
	mac TEXT NOT NULL,
	ip TEXT NOT NULL,
	-- from reverse DNS, often the name the device gave DHCP
	hostname TEXT,
	-- from the MAC's prefix (nmap's OUI list)
	vendor TEXT
);

CREATE INDEX scan_hosts_mac ON scan_hosts (mac, scan_id);
CREATE INDEX scan_hosts_scan ON scan_hosts (scan_id);
