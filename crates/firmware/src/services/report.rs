//! Posts a report to the collector: plain HTTP/1.0 on the LAN. The board has
//! no clock; the collector stamps the receive time.

use core::net::SocketAddrV4;

use embassy_net::{Stack, tcp::TcpSocket};
use embassy_time::Duration;
use embedded_io_async::Write;

use crate::{Error, Result};

const PATH: &str = "/report";
const TIMEOUT_SECS: u64 = 5;

/// Sends `body` as JSON and expects a 2xx status back.
pub async fn post(
	stack: Stack<'static>,
	addr: SocketAddrV4,
	body: &[u8],
	rx: &mut [u8],
	tx: &mut [u8],
) -> Result<()> {
	let mut socket = TcpSocket::new(stack, rx, tx);
	socket.set_timeout(Some(Duration::from_secs(TIMEOUT_SECS)));
	socket.connect((*addr.ip(), addr.port())).await.map_err(|_| Error::Report("connect"))?;

	let mut digits = [0u8; 10];
	let send = async {
		socket.write_all(b"POST ").await?;
		socket.write_all(PATH.as_bytes()).await?;
		socket
			.write_all(b" HTTP/1.0\r\nContent-Type: application/json\r\nContent-Length: ")
			.await?;
		socket.write_all(decimal(body.len(), &mut digits)).await?;
		socket.write_all(b"\r\n\r\n").await?;
		socket.write_all(body).await?;
		socket.flush().await
	};
	send.await.map_err(|_| Error::Report("send"))?;

	// "HTTP/1.x NNN": the status starts at byte 9
	let mut status = [0u8; 12];
	let mut got = 0;
	while got < status.len() {
		match socket.read(&mut status[got..]).await {
			Ok(0) | Err(_) => return Err(Error::Report("no status")),
			Ok(n) => got += n,
		}
	}
	socket.close();
	match status[9] {
		b'2' => Ok(()),
		_ => Err(Error::Report("status not 2xx")),
	}
}

fn decimal(mut n: usize, buf: &mut [u8; 10]) -> &[u8] {
	let mut at = buf.len();
	loop {
		at -= 1;
		buf[at] = b'0' + (n % 10) as u8;
		n /= 10;
		if n == 0 {
			return &buf[at..];
		}
	}
}
