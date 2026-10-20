-- Names the owner gives devices, by MAC, shown instead of the sweep's
-- hostname or the MAC: a row of smart plugs all sweep as the same name.
-- See docs/floorplan.md, chunk 9.

CREATE TABLE device_names (
	mac TEXT PRIMARY KEY,
	name TEXT NOT NULL,
	named_at INTEGER NOT NULL
);
