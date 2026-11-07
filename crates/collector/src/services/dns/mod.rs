//! DNS servers with a query log. Each file here speaks one server's API and
//! answers in core's types; see `docs/dns.md`.

use crate::{
	Result,
	core::models::dns::{DnsClient, DnsQuery},
};

pub mod pihole;

/// What every DNS server is asked. Generic at the caller (`L: DnsLog`), one
/// type picked at startup, so the futures stay `Send` for `tokio::spawn`.
pub trait DnsLog {
	/// The queries with `from <= at < until`. `until` is exclusive, so windows
	/// that follow each other read every query exactly once.
	fn queries(
		&mut self,
		from: i64,
		until: i64,
	) -> impl Future<Output = Result<Vec<DnsQuery>>> + Send;

	/// The server's own table of addresses and MACs.
	fn clients(&mut self) -> impl Future<Output = Result<Vec<DnsClient>>> + Send;

	/// Lets go of whatever the server holds open for this client.
	fn close(&mut self) -> impl Future<Output = Result<()>> + Send {
		async { Ok(()) }
	}
}
