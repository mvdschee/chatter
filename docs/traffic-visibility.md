# Seeing all Wi-Fi traffic, not just what the sensor can decode

Written 2026-10-16, after the download test. The router model and other
household details stay out of this repo on purpose.

## What the download test showed

A MacBook on the watched 2.4 GHz network downloaded 100 MB. The sensor saw
none of it: only the Mac's small keep-alive frames (28 B each), which are sent
single-stream at basic rates. No device moved more than 13 KiB in any 10 s
period.

The C6 and the C3 each have one antenna and one receive chain. A 2- or
3-stream MIMO frame (802.11n/ac/ax) can only be decoded by a receiver with at
least that many chains, sampled on one clock. Several single-antenna boards
can't be combined into one, so adding boards doesn't fix this. 802.11ax also
splits a channel between clients (OFDMA, MU-MIMO), which even multi-antenna
sniffers struggle to follow.

**Conclusion:** sniffing the air gives byte counts only for single-stream
clients. Most cheap IoT gear is single-stream, so the sensor still counts those.
Phones, tablets and laptops are not.

## Where byte-accurate per-device traffic comes from

At a point every packet passes after Wi-Fi decryption: the router, or a host
acting as gateway. That's the standard approach. Constraint here: **no new
hardware** in front of the network.

| Option | What it gives | Cost and risk |
|---|---|---|
| Router's own stats | Per-client counters, if the UI or an API has them | None. But the vendor docs list only TR-069/TR-181 (ISP management), not SNMP or TR-064, and no per-client traffic API was found. **Still to check by hand in the web UI.** |
| OpenWrt on the existing router | `nlbwmon`/conntrack: exact bytes per device, both bands | **High.** The ISP firmware is locked down. Some firmware builds block flashing, a failed flash can brick the router beyond serial recovery, and users report the ISP connection breaking afterwards. Not recommended. |
| Home server as gateway, via DHCP | nftables counters per device for all internet traffic, every band and stream count, exact | Medium. The home server becomes a single point of failure for the internet. Device-to-device LAN traffic skips it. Router IPv6 (RA) can bypass it unless turned off. **Only practical if Pi-hole or the home server does DHCP.** |
| DNS log (exists) | Who talks to which names, per client | None. No byte counts. |
| Air sensor (this repo) | Single-stream devices: bytes, retries, RSSI, presence; plus broadcast totals | Built. Blind to MIMO clients. |

## Recommended split

- **Exact bytes for everything:** the home server as gateway, if DHCP allows
  it. Otherwise only what the router UI shows.
- **Radio layer for IoT:** the sensor. It covers what the gateway can't see:
  RSSI, retries, airtime, presence, and deauth or rogue-AP activity.
- **Names:** the DNS log.

## Answers so far (2026-10-16)

- The router's web UI has no per-device traffic page.
- The router serves DHCP; Pi-hole is DNS only.

So the gateway route goes through Pi-hole's own DHCP server (dnsmasq, built
in):

- Router DHCP off, Pi-hole DHCP on. Two DHCP servers on one network conflict.
- Pi-hole hands **only the IoT devices** the home server as gateway, by MAC
  (`dhcp-host=<mac>,set:iot` plus `dhcp-option=tag:iot,option:router,<home server>`).
  Phones and laptops keep the router, so a home server outage only cuts IoT
  devices off the internet. New DHCP leases fail for everyone while it's down.
- The home server forwards (`net.ipv4.ip_forward=1`) and counts bytes per
  device with nftables.
- IPv6: router advertisements from the router would let IoT devices bypass
  the gateway. Check whether the router can turn them off.
- This belongs in the infrastructure repo (the home server's Ansible role),
  not here.

## Still unsure

1. Can the router's DHCP server be turned off, and its IPv6 router
   advertisements?
2. Does each IoT device keep a fixed MAC? Rotating MACs fall out of the
   `iot` tag.

## Validating the sensor

The Mac can't validate the counts, being MIMO. Use an ESP32-C3 (single-stream,
2.4 GHz) as a test client: it downloads a file of known size from the home
server, and the sensor's rx bytes for its MAC should match within the Wi-Fi
overhead.

## Sources

The OpenWrt wiki and forum pages for this router model, its datasheet, and a vendor
community thread on per-client statistics (checked 2026-10-16). The
links name the model, so they stay out of the repo.
