-- How many streams each device says it can use, by MAC (docs/streams.md).
-- The sensor forgets at every boot, and a device rarely asks to associate,
-- so this outlives the reports: no reference to them.

CREATE TABLE device_streams (
	mac TEXT PRIMARY KEY,
	streams INTEGER NOT NULL CHECK (streams BETWEEN 1 AND 8),
	streams_from TEXT NOT NULL CHECK (streams_from IN ('assoc', 'probe')),
	-- when a report first had a count for it, from either source
	first_at INTEGER NOT NULL,
	-- when a report first had an association request's count; NULL until then
	assoc_at INTEGER
);
