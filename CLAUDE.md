# chatter

## Override of the global rules

- **Claude writes `crates/core/` in this project**, both `crates/core/src/models/` and
  `crates/core/src/handlers/`, and the collector's `crates/collector/src/core/`
  too. This replaces "The Core Rule" in the global CLAUDE.md. Everything else
  there still applies.
- Work in small, reviewable chunks, one commit each, following the plan in
  `docs/sensor.md`.
- **This repo is public.** Nothing in it may describe the owner's own network
  or household: no MACs, IPs, SSIDs, hostnames, router model or ISP, device
  inventory or counts, channel numbers, test hardware, or the software they
  run. Write findings generically ("a multi-stream laptop", "the router's
  channel"). Real values only live in the git-ignored `.env` or in `/tmp`.
  Made-up example data (MACs, names, addresses) is generated at random, never
  adapted from real values.
  Before staging, scan the changed docs for this. Ask before any web search or
  external request that would contain household equipment details.

## Where things are

- `docs/sensor.md`: design, layout, pitfalls, and the chunked plan.
- `docs/monitoring.md`: the plan for the monitoring app (floor-plan view,
  sources, outliers).
- `docs/floorplan.md`: the floor plan editor: research, data and plan.
- `docs/timeline.md`: the timeline (ranges, playback) and the sensor
  heartbeat: research and plan.
- `docs/outliers.md`: colouring devices by their own normal (baselines,
  robust score): research and plan, built.
- `docs/identify.md`: identify mode (hold the sensor next to a device to
  tell which one it is): research and plan, the next thing to build.
- `docs/streams.md`: flagging multi-stream devices, whose counts the sensor
  can't complete: theories and tests, not yet run.
- `docs/dns.md`: the names each device looks up, from a DNS filter's query
  log, optional: research and plan, the next thing to build.
- `docs/setup.md`: the install log. Add every install or setup step as it is
  run.
- `DECISIONS.md`: why things are the way they are.
