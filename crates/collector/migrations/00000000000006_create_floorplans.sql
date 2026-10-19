-- The floor plan: rooms as polygons in metres and the openings on their
-- edges, as one JSON document. Each save is a new row; the newest is the
-- plan, older ones can be restored. See docs/floorplan.md.

CREATE TABLE floorplans (
	id INTEGER PRIMARY KEY,
	plan TEXT NOT NULL,
	saved_at INTEGER NOT NULL
);
