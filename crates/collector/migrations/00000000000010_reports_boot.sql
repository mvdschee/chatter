-- A random id per sensor boot (docs/identify.md, Pitfalls): a resend is the
-- same seq from the same boot. A reboot repeats seq and uptime, so the
-- resend window of migration 8 took a reboot within the hour for a resend.
-- NULL for reports from firmware without it, which keep the window.

ALTER TABLE reports ADD COLUMN boot INTEGER;

CREATE INDEX reports_sensor_boot ON reports (sensor, boot, seq);
