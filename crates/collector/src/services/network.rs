use std::net::{IpAddr, Ipv4Addr, UdpSocket};

use crate::{Error, Result};

/// The IPv4 address this machine uses for its default route: on a home
/// network, its LAN address. A UDP "connect" makes the OS pick the route and
/// the local address; it sends nothing. 192.0.2.1 is a documentation address,
/// never local, so the default route is the one picked.
pub fn default_ipv4() -> Result<Ipv4Addr> {
	let err = |e: std::io::Error| Error::Scan(format!("no default route: {e}"));
	let socket = UdpSocket::bind("0.0.0.0:0").map_err(err)?;
	socket.connect("192.0.2.1:9").map_err(err)?;
	match socket.local_addr().map_err(err)?.ip() {
		IpAddr::V4(ip) if !ip.is_unspecified() => Ok(ip),
		ip => Err(Error::Scan(format!("no IPv4 default route (got {ip})"))),
	}
}
