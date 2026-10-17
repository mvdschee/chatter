# chatter

## Override of the global rules

- **Claude writes `crates/core/` in this project**, both `crates/core/src/models/` and
  `crates/core/src/handlers/`. This replaces "The Core Rule" in the global
  CLAUDE.md. Everything else there still applies.
- Work in small, reviewable chunks, one commit each, following the plan in
  `docs/sensor.md`.
- **This repo is public.** Nothing in it may describe the owner's own network
  or household: no MACs, IPs, SSIDs, hostnames, router model or ISP, device
  inventory or counts, channel numbers, test hardware, or the software they
  run. Write findings generically ("a multi-stream laptop", "the router's
  channel"). Real values only live in the git-ignored `.env` or in `/tmp`.
  Before staging, scan the changed docs for this. Ask before any web search or
  external request that would contain household equipment details.

## Where things are

- `docs/sensor.md`: design, layout, pitfalls, and the chunked plan.
- `docs/monitoring.md`: the plan for the monitoring app (floor-plan view,
  sources, outliers).
- `docs/setup.md`: the install log. Add every install or setup step as it is
  run.
- `DECISIONS.md`: why things are the way they are.
