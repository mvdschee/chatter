-- Identify sessions (docs/identify.md): the sensor stops counting during
-- one, so the gap in its minute reports has a reason. A row is written when
-- a session starts and its end moved when it's extended or stopped early.
-- Times are unix seconds.

CREATE TABLE identify_sessions (
	sensor TEXT NOT NULL,
	started_at INTEGER NOT NULL,
	ended_at INTEGER NOT NULL,
	PRIMARY KEY (sensor, started_at)
);
