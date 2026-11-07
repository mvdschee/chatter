//! The names each device looks up, from a DNS filter's query log. The log
//! knows an IP, the app knows devices by MAC: this joins the two, and counts
//! a device's queries per day and registrable domain. See `docs/dns.md`.

use std::{
	collections::{BTreeMap, HashMap},
	net::IpAddr,
};

use psl::{List, Psl, Type};

use crate::core::models::report::Mac;

/// A sweep older or newer than this says nothing about who has an address
/// now: a home router's lease lasts about a day.
pub const SIGHTING_MAX_AGE_S: i64 = 86_400;
/// A sweep with none on the query's other side has nothing to check it against,
/// so it has to be this close.
pub const LONE_SIGHTING_MAX_AGE_S: i64 = 3_600;
/// The log isn't read closer to now than this: a query can still be on its way
/// in, and the DNS server's clock can be a little ahead.
pub const READ_BEHIND_S: i64 = 60;
/// A restart or a long gap reads no further back than this.
pub const CATCH_UP_S: i64 = 86_400;
/// How many days of counts are kept.
pub const KEEP_DAYS: i64 = 30;
/// Fewer queries than this don't say whether the router forwards the rest.
// used from the panel's message, which comes with docs/dns.md chunk 4
#[allow(dead_code)]
pub const ROUTER_MIN_QUERIES: i64 = 100;

const SECONDS_A_DAY: i64 = 86_400;

/// A line of the DNS server's query log.
#[derive(Debug, Clone, PartialEq)]
pub struct DnsQuery {
	pub at: i64,
	pub client: IpAddr,
	pub name: String,
}

/// An address and its MAC, from the DNS server's own table. The table has no
/// history: it's what the server knows now.
#[derive(Debug, Clone, PartialEq)]
pub struct DnsClient {
	pub ip: IpAddr,
	pub mac: Mac,
}

/// A MAC that had an address in a sweep.
#[derive(Debug, Clone, PartialEq)]
pub struct Sighting {
	pub at: i64,
	pub ip: IpAddr,
	pub mac: Mac,
}

/// How many times a device looked up a domain in a day.
#[derive(Debug, Clone, PartialEq)]
pub struct DomainCount {
	pub mac: String,
	/// Whole days since the epoch, in UTC.
	pub day: i64,
	pub domain: String,
	pub queries: i64,
}

/// The queries that could be credited to a device, counted per device, day
/// and registrable domain. The rest aren't counted: a name with no domain to
/// show, or an address no MAC can be trusted for.
///
/// A query newer than the last sweep has nothing after it to be checked
/// against, so `next_window` doesn't read that far until the next sweep.
pub fn domain_counts(
	queries: &[DnsQuery],
	clients: &[DnsClient],
	sightings: &[Sighting],
) -> Vec<DomainCount> {
	let table = client_table(clients);
	let sweeps = sweep_table(sightings);

	let mut counts: BTreeMap<(&str, i64, String), i64> = BTreeMap::new();
	for query in queries {
		let Some(domain) = registrable_domain(&query.name) else {
			continue;
		};
		let client = query.client.to_canonical();
		if let Some(mac) = mac_of(client, query.at, &table, &sweeps) {
			*counts.entry((mac, day_of(query.at), domain)).or_default() += 1;
		}
	}
	counts
		.into_iter()
		.map(|((mac, day, domain), queries)| DomainCount {
			mac: mac.to_string(),
			day,
			domain,
			queries,
		})
		.collect()
}

/// The part of a name its owner registered: `a.b.example.co.uk` is
/// `example.co.uk`. Hosting suffixes count as ordinary domains, so every
/// `*.cloudfront.net` is one `cloudfront.net` rather than one row per customer.
///
/// `None` for what isn't a name to show: a single label (`printer`), a public
/// suffix on its own, a reverse lookup, a service label (`_tcp.local`).
pub fn registrable_domain(name: &str) -> Option<String> {
	// resolvers vary the case of a name, and the list only matches lower case
	let name = name.to_ascii_lowercase();
	let name = name.strip_suffix('.').unwrap_or(&name);
	if name.split('.').any(str::is_empty) || is_arpa(name) {
		return None;
	}
	let domain = icann_domain(name)?;
	(!domain.starts_with('_')).then(|| domain.to_string())
}

/// The internet's own plumbing: reverse lookups (`in-addr.arpa`, `ip6.arpa`) and
/// a device looking for a resolver (`_dns.resolver.arpa`). They say nothing about
/// what it talks to.
fn is_arpa(name: &str) -> bool {
	name == "arpa" || name.ends_with(".arpa")
}

/// The registrable domain by the list's ICANN section alone: drops labels
/// while the suffix is one from the private section (a hosting company's).
fn icann_domain(name: &str) -> Option<&str> {
	let mut tail = name;
	let mut hosted = None;
	while List.suffix(tail.as_bytes()).and_then(|s| s.typ()) == Some(Type::Private) {
		hosted = Some(tail);
		tail = tail.split_once('.')?.1;
	}
	match List.domain(tail.as_bytes()) {
		Some(domain) => std::str::from_utf8(domain.trim().as_bytes()).ok(),
		// the hosting suffix itself, as in `cloudfront.net`: `net` has no domain
		None => hosted,
	}
}

/// The DNS server's table by address. An address listed under two MACs is
/// `None`: the table can't tell which has it now.
fn client_table(clients: &[DnsClient]) -> HashMap<IpAddr, Option<&str>> {
	let mut table: HashMap<IpAddr, Option<&str>> = HashMap::new();
	for client in clients {
		table
			.entry(client.ip.to_canonical())
			.and_modify(|mac| {
				if *mac != Some(client.mac.as_str()) {
					*mac = None;
				}
			})
			.or_insert(Some(client.mac.as_str()));
	}
	table
}

/// The sweeps by address, oldest first.
fn sweep_table(sightings: &[Sighting]) -> HashMap<IpAddr, Vec<(i64, &str)>> {
	let mut sweeps: HashMap<IpAddr, Vec<(i64, &str)>> = HashMap::new();
	for s in sightings {
		sweeps.entry(s.ip.to_canonical()).or_default().push((s.at, s.mac.as_str()));
	}
	for seen in sweeps.values_mut() {
		seen.sort_unstable();
	}
	sweeps
}

/// Which device had `ip` at `at`: the sweeps, as they know when; the DNS
/// server's table has no times, so it only speaks where no sweep is near, which
/// mostly means IPv6.
fn mac_of<'a>(
	ip: IpAddr,
	at: i64,
	table: &HashMap<IpAddr, Option<&'a str>>,
	sweeps: &HashMap<IpAddr, Vec<(i64, &'a str)>>,
) -> Option<&'a str> {
	match sweeps.get(&ip).map_or(FromSweeps::Unknown, |seen| mac_between_sweeps(seen, at)) {
		FromSweeps::Mac(mac) => Some(mac),
		FromSweeps::Moved => None,
		FromSweeps::Unknown => table.get(&ip).copied().flatten(),
	}
}

/// What the sweeps say about an address at a moment.
#[derive(Debug, PartialEq)]
enum FromSweeps<'a> {
	Mac(&'a str),
	/// They name two MACs, and nothing says which had it then.
	Moved,
	/// No sweep is near enough to say.
	Unknown,
}

/// The MAC that had an address around `at`, from its sightings (oldest
/// first): the last sweep up to `at` and the first one after it, each only
/// if it's within `SIGHTING_MAX_AGE_S`, and within `LONE_SIGHTING_MAX_AGE_S` if
/// it's the only one. If they disagree the address moved somewhere between them
/// and no one can say when.
fn mac_between_sweeps<'a>(seen: &[(i64, &'a str)], at: i64) -> FromSweeps<'a> {
	let split = seen.partition_point(|&(t, _)| t <= at);
	let (before, after) = seen.split_at(split);

	// one sweep can list an address twice, under two MACs
	let last = before.last().filter(|(t, _)| at - t <= SIGHTING_MAX_AGE_S).map(|(t, _)| *t);
	// a sweep at that very second needs no second opinion
	let first = after
		.first()
		.filter(|(t, _)| t - at <= SIGHTING_MAX_AGE_S && last != Some(at))
		.map(|(t, _)| *t);
	let mut macs =
		seen.iter().filter(|(t, _)| Some(*t) == last || Some(*t) == first).map(|(_, mac)| *mac);

	let lone = match (last, first) {
		(None, None) => return FromSweeps::Unknown,
		(Some(t), None) | (None, Some(t)) => Some(t),
		(Some(_), Some(_)) => None,
	};
	if lone.is_some_and(|t| (t - at).abs() > LONE_SIGHTING_MAX_AGE_S) {
		return FromSweeps::Unknown;
	}

	let Some(mac) = macs.next() else {
		return FromSweeps::Unknown;
	};
	if macs.all(|other| other == mac) {
		FromSweeps::Mac(mac)
	} else {
		FromSweeps::Moved
	}
}

/// The whole UTC day a moment falls in, counted from the epoch.
pub fn day_of(at: i64) -> i64 {
	at.div_euclid(SECONDS_A_DAY)
}

/// A stretch of the query log, `from <= at < until`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
	pub from: i64,
	pub until: i64,
}

/// What to read next, if anything: from where the last read ended up to the last
/// sweep, as a query newer than that has no sweep after it yet. When sweeps have
/// stopped (or never ran) there's nothing to wait for. `read_until` is `None`
/// before the first read.
pub fn next_window(
	now: i64,
	read_until: Option<i64>,
	last_sweep: Option<i64>,
	sweep_every_s: i64,
) -> Option<Window> {
	let settled = now - READ_BEHIND_S;
	// a few missed sweeps is not yet stopped, however often they run
	let stopped_after = (3 * sweep_every_s).max(LONE_SIGHTING_MAX_AGE_S);
	let until = match last_sweep {
		Some(sweep) if now - sweep <= stopped_after => settled.min(sweep),
		_ => settled,
	};
	let earliest = until - CATCH_UP_S;
	let from = read_until.map_or(earliest, |read| read.max(earliest));
	(from < until).then_some(Window {
		from,
		until,
	})
}

/// The sweeps that can say something about a window's queries, `from` and
/// `until` both included.
pub fn sightings_around(window: Window) -> (i64, i64) {
	(window.from - SIGHTING_MAX_AGE_S, window.until + SIGHTING_MAX_AGE_S)
}

/// One UTC day's queries, the ones the join leaves out too.
#[derive(Debug, Clone, PartialEq)]
pub struct DayTotal {
	pub day: i64,
	pub total: i64,
	pub from_gateway: i64,
}

/// How many queries each day had, and how many of those came from the gateway;
/// none when it isn't known.
pub fn day_totals(queries: &[DnsQuery], gateway: Option<IpAddr>) -> Vec<DayTotal> {
	let gateway = gateway.map(|g| g.to_canonical());
	let mut days: BTreeMap<i64, DayTotal> = BTreeMap::new();
	for query in queries {
		let day = day_of(query.at);
		let totals = days.entry(day).or_insert(DayTotal {
			day,
			total: 0,
			from_gateway: 0,
		});
		totals.total += 1;
		totals.from_gateway += i64::from(Some(query.client.to_canonical()) == gateway);
	}
	days.into_values().collect()
}

/// Whether the router makes most of the queries: then it hands out its own
/// address as the DNS server and the filter can't see who's asking.
// used from the panel's message, which comes with docs/dns.md chunk 4
#[allow(dead_code)]
pub fn router_forwards(from_gateway: i64, total: i64) -> bool {
	total >= ROUTER_MIN_QUERIES && from_gateway * 2 > total
}

#[cfg(test)]
mod tests {
	use super::*;

	// made up, at random
	const MAC_A: &str = "e7:f2:5e:ef:88:30";
	const MAC_B: &str = "b5:11:ac:5e:9d:9c";
	const MAC_C: &str = "5c:34:82:e3:c5:d0";

	const IP: &str = "192.0.2.10";
	const HOUR: i64 = 3600;
	/// Well past the epoch, and the start of a UTC day.
	const T: i64 = 20_000 * SECONDS_A_DAY;

	fn query(at: i64, client: &str, name: &str) -> DnsQuery {
		DnsQuery {
			at,
			client: client.parse().unwrap(),
			name: name.to_string(),
		}
	}

	fn client(ip: &str, mac: &str) -> DnsClient {
		DnsClient {
			ip: ip.parse().unwrap(),
			mac: Mac::new(mac).unwrap(),
		}
	}

	fn sighting(at: i64, ip: &str, mac: &str) -> Sighting {
		Sighting {
			at,
			ip: ip.parse().unwrap(),
			mac: Mac::new(mac).unwrap(),
		}
	}

	/// Who `IP` is credited to at `at`, through the whole count.
	fn credited(at: i64, clients: &[DnsClient], sightings: &[Sighting]) -> Option<String> {
		let counts = domain_counts(&[query(at, IP, "example.com")], clients, sightings);
		counts.first().map(|c| c.mac.clone())
	}

	#[test]
	fn a_name_counts_as_its_registrable_domain() {
		for (name, domain) in [
			("a.b.example.co.uk", "example.co.uk"),
			("api.example.com", "example.com"),
			("example.com", "example.com"),
			("EXAMPLE.Com", "example.com"),
			("api.example.com.", "example.com"),
			// a suffix the list doesn't know, as a home's own names have
			("nas.lan", "nas.lan"),
		] {
			assert_eq!(registrable_domain(name).as_deref(), Some(domain), "{name}");
		}
	}

	#[test]
	fn a_hosting_suffix_is_one_domain_not_one_per_customer() {
		for (name, domain) in [
			("d1234.cloudfront.net", "cloudfront.net"),
			("cloudfront.net", "cloudfront.net"),
			("bucket.s3.eu-west-1.amazonaws.com", "amazonaws.com"),
			("someone.github.io", "github.io"),
		] {
			assert_eq!(registrable_domain(name).as_deref(), Some(domain), "{name}");
		}
	}

	#[test]
	fn what_has_no_domain_to_show_is_left_out() {
		for name in [
			"",
			"printer",
			"com",
			"co.uk",
			"a..example.com",
			"10.2.0.192.in-addr.arpa",
			"1.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.8.b.d.0.1.0.0.2.ip6.arpa",
			"lb._dns-sd._udp.2.0.192.in-addr.arpa",
			"_airplay._tcp.local",
			"_dns.resolver.arpa",
			"resolver.arpa",
			"arpa",
		] {
			assert_eq!(registrable_domain(name), None, "{name:?}");
		}
	}

	#[test]
	fn the_sweeps_win_over_the_servers_table() {
		let table = [client(IP, MAC_A)];
		let sweeps = [sighting(T - 60, IP, MAC_B)];
		assert_eq!(credited(T, &table, &sweeps).as_deref(), Some(MAC_B));
	}

	#[test]
	fn an_address_that_moved_does_not_fall_back_to_the_table() {
		let table = [client(IP, MAC_A)];
		let sweeps = [sighting(T - 60, IP, MAC_A), sighting(T + 60, IP, MAC_B)];
		assert_eq!(credited(T, &table, &sweeps), None);
	}

	#[test]
	fn the_table_speaks_where_no_sweep_is_near() {
		let table = [client(IP, MAC_A)];
		assert_eq!(credited(T, &table, &[]).as_deref(), Some(MAC_A));

		let far = [sighting(T - LONE_SIGHTING_MAX_AGE_S - 1, IP, MAC_B)];
		assert_eq!(credited(T, &table, &far).as_deref(), Some(MAC_A));
		let near = [sighting(T - LONE_SIGHTING_MAX_AGE_S, IP, MAC_B)];
		assert_eq!(credited(T, &table, &near).as_deref(), Some(MAC_B));
	}

	#[test]
	fn an_address_the_table_lists_twice_is_left_to_the_sweeps() {
		let table = [client(IP, MAC_A), client(IP, MAC_B)];
		assert_eq!(credited(T, &table, &[]), None);

		let sweeps = [sighting(T - 60, IP, MAC_C)];
		assert_eq!(credited(T, &table, &sweeps).as_deref(), Some(MAC_C));

		// the same pair twice isn't a conflict
		let table = [client(IP, MAC_A), client(IP, MAC_A)];
		assert_eq!(credited(T, &table, &[]).as_deref(), Some(MAC_A));
	}

	#[test]
	fn a_lease_that_moves_between_sweeps() {
		let moved = T + 10 * 60;
		let sweeps = [
			sighting(T - 10 * 60, IP, MAC_A),
			sighting(T, IP, MAC_A),
			sighting(moved, IP, MAC_B),
			sighting(moved + 10 * 60, IP, MAC_B),
		];

		assert_eq!(credited(T - 60, &[], &sweeps).as_deref(), Some(MAC_A));
		// on a sweep's own second, that sweep counts
		assert_eq!(credited(T, &[], &sweeps).as_deref(), Some(MAC_A));
		// somewhere in here it moved, and nothing says when
		assert_eq!(credited(T + 1, &[], &sweeps), None);
		assert_eq!(credited(moved - 1, &[], &sweeps), None);
		assert_eq!(credited(moved, &[], &sweeps).as_deref(), Some(MAC_B));
		assert_eq!(credited(moved + 60, &[], &sweeps).as_deref(), Some(MAC_B));
	}

	#[test]
	fn the_sweep_after_a_query_counts_too() {
		let sweeps = [sighting(T + 60, IP, MAC_A)];
		assert_eq!(credited(T, &[], &sweeps).as_deref(), Some(MAC_A));
	}

	#[test]
	fn one_sweep_with_an_address_under_two_macs_is_left_out() {
		let sweeps = [sighting(T - 60, IP, MAC_A), sighting(T - 60, IP, MAC_B)];
		assert_eq!(credited(T, &[], &sweeps), None);
	}

	#[test]
	fn a_sighting_too_far_away_says_nothing() {
		let limit = LONE_SIGHTING_MAX_AGE_S;

		let old = [sighting(T - limit, IP, MAC_A)];
		assert_eq!(credited(T, &[], &old).as_deref(), Some(MAC_A));
		let old = [sighting(T - limit - 1, IP, MAC_A)];
		assert_eq!(credited(T, &[], &old), None);

		let later = [sighting(T + limit, IP, MAC_A)];
		assert_eq!(credited(T, &[], &later).as_deref(), Some(MAC_A));
		let later = [sighting(T + limit + 1, IP, MAC_A)];
		assert_eq!(credited(T, &[], &later), None);

		// a far sighting of another MAC doesn't spoil a near one, either way
		let day = SIGHTING_MAX_AGE_S;
		let both = [sighting(T - HOUR, IP, MAC_A), sighting(T + day + 1, IP, MAC_B)];
		assert_eq!(credited(T, &[], &both).as_deref(), Some(MAC_A));
		let both = [sighting(T - day - 1, IP, MAC_B), sighting(T + HOUR, IP, MAC_A)];
		assert_eq!(credited(T, &[], &both).as_deref(), Some(MAC_A));
	}

	#[test]
	fn sweeps_on_both_sides_may_be_far_if_they_agree() {
		let day = SIGHTING_MAX_AGE_S;
		// a phone asleep through the sweeps in between
		let both = [sighting(T - day, IP, MAC_A), sighting(T + day, IP, MAC_A)];
		assert_eq!(credited(T, &[], &both).as_deref(), Some(MAC_A));

		let moved = [sighting(T - day, IP, MAC_A), sighting(T + day, IP, MAC_B)];
		assert_eq!(credited(T, &[], &moved), None);
	}

	#[test]
	fn a_window_ends_at_the_last_sweep() {
		const EVERY: i64 = 600;
		let window = |from, until| {
			Some(Window {
				from,
				until,
			})
		};
		// up to the sweep, and no nearer to now than READ_BEHIND_S
		assert_eq!(next_window(T + 300, Some(T - 600), Some(T), EVERY), window(T - 600, T));
		assert_eq!(
			next_window(T + 30, Some(T - 600), Some(T), EVERY),
			window(T - 600, T + 30 - 60)
		);
		// nothing new since the last read
		assert_eq!(next_window(T + 300, Some(T), Some(T), EVERY), None);
		// sweeps that started up again are behind what was read
		assert_eq!(next_window(T + 300, Some(T + 100), Some(T), EVERY), None);
		// a first read, and a long gap
		assert_eq!(next_window(T + 300, None, Some(T), EVERY), window(T - CATCH_UP_S, T));
		assert_eq!(
			next_window(T + 300, Some(T - 90 * HOUR), Some(T), EVERY),
			window(T - CATCH_UP_S, T)
		);
	}

	#[test]
	fn a_window_stops_waiting_when_sweeps_have_stopped() {
		const EVERY: i64 = 600;
		let until = |now, every| next_window(now, Some(T - 600), Some(T), every).map(|w| w.until);
		let hour = LONE_SIGHTING_MAX_AGE_S;
		assert_eq!(until(T + hour, EVERY), Some(T));
		assert_eq!(until(T + hour + 1, EVERY), Some(T + hour + 1 - READ_BEHIND_S));
		// sweeps two hours apart aren't late after one
		let two_hours = 2 * hour;
		assert_eq!(until(T + 6 * hour, two_hours), Some(T));
		assert_eq!(until(T + 6 * hour + 1, two_hours), Some(T + 6 * hour + 1 - READ_BEHIND_S));
		// and never a sweep at all
		let never = next_window(T, Some(T - 600), None, EVERY).map(|w| w.until);
		assert_eq!(never, Some(T - READ_BEHIND_S));
	}

	#[test]
	fn ipv6_needs_the_table() {
		let ip = "2001:db8::10";
		let name = "example.com";
		let q = [query(T, ip, name)];

		assert!(domain_counts(&q, &[], &[sighting(T, IP, MAC_A)]).is_empty());

		let counts = domain_counts(&q, &[client(ip, MAC_A)], &[]);
		assert_eq!(counts.len(), 1);
		assert_eq!(counts[0].mac, MAC_A);
	}

	#[test]
	fn an_ipv4_address_written_as_ipv6_is_the_same_address() {
		let q = [query(T, "::ffff:192.0.2.10", "example.com")];

		let counts = domain_counts(&q, &[], &[sighting(T, IP, MAC_A)]);
		assert_eq!(counts[0].mac, MAC_A);

		let counts = domain_counts(&q, &[client(IP, MAC_A)], &[]);
		assert_eq!(counts[0].mac, MAC_A);

		let table = [client("::ffff:192.0.2.10", MAC_A)];
		assert_eq!(credited(T, &table, &[]).as_deref(), Some(MAC_A));
	}

	#[test]
	fn counts_are_per_device_day_and_domain() {
		let other = "192.0.2.11";
		let last_second = T + SECONDS_A_DAY - 1;
		let queries = [
			query(T, IP, "api.example.com"),
			query(last_second, IP, "cdn.example.com"),
			query(last_second + 1, IP, "example.com"),
			query(T, IP, "example.org"),
			query(T, other, "example.com"),
			// neither of these counts
			query(T, IP, "printer"),
			query(T, "192.0.2.99", "example.com"),
		];
		let clients = [client(IP, MAC_A), client(other, MAC_B)];

		let count = |mac: &str, day: i64, domain: &str, queries: i64| DomainCount {
			mac: mac.to_string(),
			day,
			domain: domain.to_string(),
			queries,
		};
		let day = T / SECONDS_A_DAY;
		assert_eq!(
			domain_counts(&queries, &clients, &[]),
			vec![
				count(MAC_B, day, "example.com", 1),
				count(MAC_A, day, "example.com", 2),
				count(MAC_A, day, "example.org", 1),
				count(MAC_A, day + 1, "example.com", 1),
			]
		);
	}

	#[test]
	fn the_router_forwards_when_it_makes_most_of_the_queries() {
		assert!(router_forwards(51, ROUTER_MIN_QUERIES));
		assert!(!router_forwards(50, ROUTER_MIN_QUERIES));
		// too few to say
		assert!(!router_forwards(ROUTER_MIN_QUERIES - 1, ROUTER_MIN_QUERIES - 1));
		assert!(!router_forwards(0, 0));
	}

	#[test]
	fn days_count_every_query_and_the_ones_from_the_gateway() {
		let gateway = "192.0.2.1";
		let today = day_of(T);
		let tomorrow = (today + 1) * 86_400;
		let queries = [
			query(T, gateway, "example.com"),
			query(T, "::ffff:192.0.2.1", "example.org"),
			query(T, IP, "example.com"),
			query(tomorrow, IP, "printer"),
		];
		let day = |day, total, from_gateway| DayTotal {
			day,
			total,
			from_gateway,
		};
		assert_eq!(
			day_totals(&queries, gateway.parse().ok()),
			[day(today, 3, 2), day(today + 1, 1, 0)]
		);
		// it can't always be looked up
		assert_eq!(day_totals(&queries, None), [day(today, 3, 0), day(today + 1, 1, 0)]);
	}
}
