# DNS names per device (research and plan)

Status, 2026-11-06: chunk 1 built (the join and the counts, in
`crates/collector/src/core/models/dns.rs`) and the Pi-hole half of chunk 2 (in
`crates/collector/src/services/dns/`); nothing calls them yet. This is
`docs/monitoring.md` plan step 8.

## Goal

The device panel shows the names a device looks up: a plug that asks for
its maker's cloud, a speaker that asks for a streaming service, a laptop
that asks for everything. That tells devices apart where a MAC, a vendor
and an `ESP_3A1F2B` hostname don't, and gives a hint the sensor can't:
whom a device talks to, not just how much.

It's **optional**. Plenty of homes have no DNS filter, and the app must
work the same without one: no setting, no requests, no rows, no empty
section in the panel. Nothing else (names, outliers, identify, the
timeline) may depend on it.

## How others do it

- **The query log is the standard source.** A DNS filter on the network
  (Pi-hole, AdGuard Home and the like) logs every query with the client's
  address. NetAlertX reads Pi-hole's database and its DHCP leases to find
  devices its ARP scan missed
  ([NetAlertX](https://github.com/jokob-sk/NetAlertX)).
- **Domains tell devices apart.** In a study of smart-home traffic, 4 of 7
  devices could be told apart by their DNS queries alone, and the rest by
  the set of domains rather than any one
  ([Apthorpe et al., 2017](https://arxiv.org/pdf/1708.05044)). IoTFinder
  matches DNS fingerprints of over 50 IoT devices
  ([IoTFinder, 2020](https://astrolavos.gatech.edu/articles/IoTFinder.pdf)).
  Classifying devices from them is research work; showing a device's top
  domains, for a person to read, is the cheap half and enough here.
- **Pi-hole v6** has a REST API
  ([auth](https://docs.pi-hole.net/api/auth/),
  [queries spec](https://github.com/pi-hole/FTL/blob/master/src/api/docs/content/specs/queries.yaml)):
  - Log in with `POST /api/auth` and a password (an app password works,
    also with 2FA on); the answer is a session id, sent as the `X-FTL-SID`
    header. Sessions expire unless used, and there's a limit on how many
    are open ("API seats exceeded"), so the app keeps one, renews it, and
    logs out (`DELETE /api/auth`) when it stops.
  - `GET /api/queries` with `from` (inclusive), `until` (exclusive), `length`
    (100 by default, 10,000 at most), `start`, `cursor` and filters
    (`client_ip`, `domain`, …). Each query has `time`, `type`, `domain`,
    `client.ip`, `client.name`, `status` and `id`. Newest first. The spec reads
    as if `cursor` points at the next page, but FTL's source sends the same one
    back: it pins the first page, and `start` is what walks on.
  - `GET /api/network/devices` lists what Pi-hole has seen: `hwaddr` (the
    MAC), `macVendor`, `numQueries`, and `ips[]` with `ip` (IPv4 and IPv6),
    `name` and `lastSeen`. It's the IP-to-MAC table, IPv6 included. Without
    `max_devices` and `max_addresses` it stops at 10 devices of 3 addresses.
    `hwaddr` is `ip-<address>` for a client whose MAC it can't see, and all
    zeros for a loopback.
- **AdGuard Home** has a REST API under `/control`
  ([openapi.yaml](https://github.com/AdguardTeam/AdGuardHome/blob/master/openapi/openapi.yaml)),
  with HTTP Basic auth:
  - `GET /control/querylog` with `older_than` or `offset`, `limit`, and
    `search` (a domain or client IP). Each entry has `time`,
    `question.name`, `question.type`, `client` (the IP) and
    `client_info.name`.
  - `GET /control/clients` lists `auto_clients` (`ip`, `name`, `source`).
    The MAC is only there when AdGuard serves DHCP (`/control/dhcp/status`).
- **Plain dnsmasq or Unbound** only log to a file or syslog. Reading a log
  file from another machine is a different job; left out (see below).

## The approach

- **One trait, two adapters.** `DnsLog` with the one action they share:
  queries since a cursor, and the client table (IP, MAC if known, name).
  Pi-hole and AdGuard Home each implement it in `services/dns/`. A third
  server is a third file.
- **Settings**: `CHATTER_DNS` (`pihole+http://…` or `adguard+http://…`, no
  space: `.env` can't hold an unquoted one, and stops reading at that line)
  and `CHATTER_DNS_PASSWORD` (for AdGuard, `user:password`). Without
  `CHATTER_DNS` nothing starts. Pi-hole's HTTPS has a self-signed certificate
  by default, which the client refuses, so its address is `http://`; the
  password then crosses the LAN readable, which is a reason to use an app
  password.
- **Polling**: every minute, one window of time, `from <= query < until` in
  whole seconds (Pi-hole `from` and `until`, AdGuard backwards with
  `older_than`), paged until done. Not one request per device. The next window
  starts where the last ended, so each query is read once, whatever else shares
  its second. `until` stays a little behind now, for a server's clock that's
  ahead and for a query still on its way into the log. Where the last window
  ended is kept in the database, so a restart picks up there; after a long gap
  it reads at most the last day.
- **The join to a MAC** is the hard part. The log knows an IP, the app
  knows devices by MAC.
  1. The DNS server's own table first: Pi-hole's `network/devices` maps
     IPv4 and IPv6 addresses to MACs; AdGuard's DHCP leases do when it
     serves DHCP.
     An address the table lists under two MACs goes on to the sweeps: the
     table has no history, so it can't say which had it at the query's time.
  2. Else the stored sweeps (`scan_hosts`), as leases move: the last sweep
     up to the query's time and the first one after it, each only if it's
     within a day. If both name one MAC, or only one is that close, it's
     that MAC. If they disagree the address moved somewhere between them,
     and the query is left out rather than guessed.
  3. Else the query isn't counted. Queries over IPv6 from a server without
     a MAC table mostly end up here.
- **What's stored**: per device per day (UTC), a registrable domain
  (`api.example.com` and `cdn.example.com` both count as `example.com`)
  and how many queries. Not the queries themselves: a log is large, and a
  home's browsing history shouldn't be kept longer than the DNS server
  keeps it. The registrable part comes from the Public Suffix List, which
  the `psl` crate compiles in, so nothing is fetched. Kept 30 days.
- **Which names count**: only the list's ICANN section decides the domain,
  so `d1234.cloudfront.net` counts as `cloudfront.net` rather than one row
  per customer. Left out: single labels (`printer`), a public suffix on its
  own, reverse lookups (`in-addr.arpa`, `ip6.arpa`) and service labels
  (`_tcp.local`). Names are lower-cased first: resolvers vary the case, and
  the list only matches lower case.
- **The panel**: a "Looks up" section on a device's panel, its top
  domains over the last 7 days with a count each, the rest as "and 40
  more". Missing when there's no DNS source; "nothing seen" when there is
  one and the device asked nothing. The list gives an idea; for the details
  (full names, single queries) a link opens the DNS server's own query log,
  filtered to the device's address where the server allows it. The app
  keeps none of that itself.

## Pitfalls

- **Every query from the router.** When the router hands out its own
  address as the DNS server and forwards to the filter, the filter sees
  every query coming from the router, and nothing can be credited to a
  device ([Pi-hole's FAQ thread](https://discourse.pi-hole.net/t/why-do-i-only-see-my-routers-ip-address-instead-of-individual-devices-in-the-top-clients-section-and-query-log/3653)).
  The app knows the gateway's address (the sweep looks it up); when it
  makes most of the queries (over half, of at least 100, counted before the
  join so queries left out there can't skew it), the router's panel says why
  and what fixes it (the router handing out the filter's address over DHCP),
  instead of the router "looking up" everything.
- **Devices that skip the filter.** Some devices use a public resolver
  they have built in, and browsers and phones may use DNS over HTTPS. They
  show few or no names, which doesn't mean they're quiet: the sensor's
  counts say how much they send.
- **Private and rotating MACs.** A phone's made-up MAC is the one the
  sweep and the DNS server both see, so the join still holds. A MAC that
  rotates starts over as a new device, as everywhere else in the app.
- **A server that can't see MACs.** Pi-hole fills its table from the neighbour
  table of the machine it runs on. Where that shows it none of the LAN (in a
  container on its own bridge network, most likely), nearly every `hwaddr` is
  `ip-<address>` and the table is no use, so the sweeps do the whole join. IPv4
  still lands; IPv6 queries aren't counted.
- **Pi-hole's sessions.** Logging in on every poll fills its seats, so one
  session is kept and renewed when it's refused.
- **Leases move.** The sweep runs every few minutes, so an address can
  change hands between two sweeps. Those minutes are left out, which takes
  a sweep after the query: a query newer than the last sweep has to wait for
  the next one (chunk 3). The server's own table has no history, so reading
  far back after a long gap can still credit a moved address to its new
  device.
- **The filter itself.** Its own lookups (blocklist updates, upstream
  checks) come from its own address, which is a device too. Fine: they're
  its names.
- **What it says about a home.** Domains per device are about as private as
  data gets. They stay in the app's own database and never go to an
  outside service (no lookups to classify a domain).

## Chunks

One commit each.

1. **Core** (built): the join (a query's IP and time to a MAC, from the DNS
   server's table, else the sweeps), the registrable domain, the count per
   device, day and domain, and the router check.
   Check: host tests on made-up data: a lease that moves between sweeps,
   an address two MACs had, IPv6 with and without a table, a router that
   makes most of the queries, `a.b.example.co.uk` counting as
   `example.co.uk`.
2. **Adapters**: `DnsLog`, Pi-hole (built) and AdGuard Home in
   `services/dns/`, parsing the APIs' JSON into core's query and client types;
   the session for Pi-hole. One commit each.
   Check: tests on made-up answers, against a stand-in server that behaves as
   the sources do, and a real server answering from the Mac (each adapter's
   ignored `live` test, which prints counts only).
3. **Polling and storage**: `CHATTER_DNS`, a migration for the daily
   counts, the last query's time and the router check's totals, the poller,
   30 days kept. Queries joined through the sweeps wait for the sweep after
   them (`scans.finished_at` is a sighting's time), unless sweeps have
   stopped.
   Check: without `CHATTER_DNS` the collector makes no request and writes
   no row (a test); with it, an hour of the owner's queries lands on the
   right devices.
4. **Panel**: "Looks up" on a device's panel, the link to the DNS server's
   query log, the router's message, a layout check scene with made-up
   domains.
   Check: the layout check at both widths; the owner reads a few devices'
   lists.
5. **Names from the DNS server**: the client names it
   knows (Pi-hole's `ips[].name`, AdGuard's `auto_clients`) fill in the
   hostname where the sweep's reverse DNS found none.
   Check: devices that were a MAC only get a name.

## Validation

Checked automatically:

- builds, clippy, host tests for the join, the domains and the router
  check;
- no DNS setting: no requests, no rows, no section in the panel;
- the adapters read the specs' shapes; the layout check with a long list.

Only the owner can judge:

- whether the top domains help tell devices apart, or are mostly noise
  (CDNs, time servers);
- whether 7 days and a top list is the right amount to show;
- whether having browsing per device in the app feels acceptable.

## Still unsure

1. How long Pi-hole keeps queries in memory, and when `disk=true` is
   needed for a gap longer than that.
2. Whether AdGuard's `search` matches a client exactly or as a substring;
   the poller doesn't filter by client, so it only matters for later.
3. Whether noise domains (time servers, connectivity checks, the big CDNs)
   should be left out of the top list, and by what list.
4. Log files (dnsmasq, Unbound) for homes without an API: only if someone
   asks.
5. The query log's address per server, for the panel's link. From memory,
   to check in chunk 4: Pi-hole `/admin/queries?client_ip=…`, AdGuard Home
   `/#logs?search=…`. The link's base is `CHATTER_DNS`, so it only opens
   where the browser can reach that address.
6. Whether Pi-hole's `ips[].lastSeen` should settle an address it lists under
   two MACs, for queries from around that time, rather than leaving it to the
   sweeps. It's only the latest sighting per MAC, not a history.
