-- What a DNS server's query log says, per device and day. Only the counts, not
-- the queries: the DNS server keeps those. `day` is whole days since the epoch,
-- in UTC. See docs/dns.md.

CREATE TABLE dns_domains (
	mac TEXT NOT NULL,
	day INTEGER NOT NULL,
	-- the registrable domain
	domain TEXT NOT NULL,
	queries INTEGER NOT NULL,
	PRIMARY KEY (mac, day, domain)
);

CREATE INDEX dns_domains_day ON dns_domains (day);

-- Every query of a day, counted before the join: says whether the router makes
-- most of them.
CREATE TABLE dns_days (
	day INTEGER PRIMARY KEY,
	total INTEGER NOT NULL,
	from_gateway INTEGER NOT NULL
);

-- How far the log has been read (unix seconds), one row. It moves in the same
-- transaction as the counts, so a query is counted once.
CREATE TABLE dns_poll (
	id INTEGER PRIMARY KEY CHECK (id = 1),
	read_until INTEGER NOT NULL
);
