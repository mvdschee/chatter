# DNS names per device (research and plan)

Status, 2026-11-08: chunks 1, 3 and 4 built (the join and the counts, the
polling and the storage, the panel), and the Pi-hole half of chunk 2 (in
`crates/collector/src/services/dns/`). This is `docs/monitoring.md` plan
step 8.

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
- **Polling**: every minute it looks for one window of time to read,
  `from <= query < until` in whole seconds (Pi-hole `from` and `until`, AdGuard
  backwards with `older_than`), paged until done. Not one request per device.
  The next window starts where the last ended, so each query is read once,
  whatever else shares its second. A window ends at the last sweep, as a newer
  query has no sweep after it yet, so with sweeps every 10 minutes the server
  gets a request every 10 minutes and the counts are that far behind. `until`
  also stays a minute behind now, for a server's clock that's ahead and for a
  query still on its way into the log. Sweeps count as stopped after an hour, or
  three missed sweeps if that's longer, and then the windows no longer wait.
  Where the last window ended is stored with its counts, in one transaction, so
  a restart or a failure picks up there; after a long gap it reads at most the
  last day.
- **The join to a MAC** is the hard part. The log knows an IP, the app
  knows devices by MAC.
  An address is only good for a time: after a DHCP release it can be another
  device. So the time and the address decide together.
  1. The stored sweeps (`scan_hosts`) first: the last sweep up to the query's
     time and the first one after it, each only if it's within a day. If both
     name one MAC, it's that MAC. If only one is that close, it has to be within
     an hour, with nothing to check it against. If they disagree the address
     moved somewhere between them, and the query is left out rather than
     guessed; the table doesn't get a say.
  2. Else, with no sweep near, the DNS server's own table: Pi-hole's
     `network/devices` maps IPv4 and IPv6 addresses to MACs; AdGuard's DHCP
     leases do when it serves DHCP. The table has no history, so it comes second,
     and mostly answers for IPv6, which the sweeps don't see. An address it lists
     under two MACs gets no answer.
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
  own, everything under `.arpa` (reverse lookups, and `_dns.resolver.arpa`, a
  device looking for a resolver) and service labels (`_tcp.local`). Names are lower-cased first: resolvers vary the case, and
  the list only matches lower case.
- **The panel**: a "DNS queries, last 7 days" section on a device's panel
  (the page names the measurement, not what the device does), its top 10
  domains over the last 7 UTC days up to the newest report's, with a count
  each, the rest as "and 40 more". Missing when there's no DNS source; "no
  queries credited to it" when there is one and none were: the join leaves
  some out, so a device can have queries in the log and none here. It comes with `/v1/overview`, per device, so the page needs no
  request of its own. The list gives an idea; for the details (full names,
  single queries) a link opens the DNS server's own query log, filtered to
  the device's current address: Pi-hole's `/admin/queries?client_ip=…`. The
  address only, not the time the device had it: Pi-hole's page reads `from`
  and `until` from its URL too, but by its source not on the first load, so
  the link would claim a range it doesn't apply. Earlier queries there may be
  another device's. The app keeps none of that itself.

## Pitfalls

- **Every query from the router.** When the router hands out its own
  address as the DNS server and forwards to the filter, the filter sees
  every query coming from the router, and nothing can be credited to a
  device ([Pi-hole's FAQ thread](https://discourse.pi-hole.net/t/why-do-i-only-see-my-routers-ip-address-instead-of-individual-devices-in-the-top-clients-section-and-query-log/3653)).
  The app knows the gateway's address (the sweep looks it up); when it
  makes most of the queries (over half, of at least 100, counted before the
  join so queries left out there can't skew it, over the 7 days shown), every
  device's DNS section says why and what fixes it (the router handing out the
  filter's address over DHCP). Every device's, not only the router's: the
  overview would need the gateway's address, which isn't stored, and the
  devices with few or no queries are where the reason is wanted.
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
  the next one. Where no sweep is near, the server's own table can still credit
  a moved address to its new device.
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
3. **Polling and storage** (built): `CHATTER_DNS`, a migration for the daily
   counts, how far the log was read and the router check's totals, the poller,
   30 days kept. A window ends at the last sweep (`scans.finished_at` is a
   sighting's time), unless sweeps have stopped.
   Check: without `CHATTER_DNS` the collector makes no request and writes
   no row (a test); with it, an hour of the owner's queries lands on the
   right devices.
4. **Panel** (built): "DNS queries, last 7 days" on a device's panel, the
   link to the DNS server's query log, the router's message, made-up
   domains in the layout check and a scene without a DNS server.
   Check: the layout check at both widths; the owner reads a few devices'
   lists (after a deploy: it needs the server's counts).
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
5. AdGuard Home's query log address for the panel's link, from memory
   `/#logs?search=…`; check with its adapter. Pi-hole's is settled (see the
   panel). The link's base is `CHATTER_DNS`, so it only opens where the
   browser can reach that address, and Pi-hole asks its own login first.
6. Whether Pi-hole's `ips[].lastSeen` should limit its table, the way the hour
   limits a lone sweep. It's the latest sighting per MAC, not a history.
