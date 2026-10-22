//! Talks to the collector: plain HTTP/1.0 on the LAN, one connection per
//! request. The board has no clock; the collector stamps the receive time.

use core::net::SocketAddrV4;

use embassy_net::{Stack, tcp::TcpSocket};
use embassy_time::Duration;
use embedded_io_async::Write;

use crate::{Error, Result};

// no ACK for this long aborts the connection; Wi-Fi in promiscuous mode can be slow
const TIMEOUT_SECS: u64 = 15;

/// Where the collector is, and the socket buffers to reach it with.
pub struct Collector<'a> {
	pub stack: Stack<'static>,
	pub addr: SocketAddrV4,
	pub rx: &'a mut [u8],
	pub tx: &'a mut [u8],
}

/// Sends `body` as JSON to `path` and expects a 2xx status back. The answer's
/// body is copied to the start of `answer` and its length returned; `answer`
/// must hold the whole response, headers included.
pub async fn request(
	collector: &mut Collector<'_>,
	method: &str,
	path: &str,
	body: &[u8],
	answer: &mut [u8],
) -> Result<usize> {
	let mut socket = TcpSocket::new(collector.stack, collector.rx, collector.tx);
	socket.set_timeout(Some(Duration::from_secs(TIMEOUT_SECS)));
	let addr = collector.addr;
	socket.connect((*addr.ip(), addr.port())).await.map_err(|_| Error::Report("connect"))?;

	let mut digits = [0u8; 10];
	let send = async {
		socket.write_all(method.as_bytes()).await?;
		socket.write_all(b" ").await?;
		socket.write_all(path.as_bytes()).await?;
		// the collector closes after answering, which ends the answer
		socket
			.write_all(
				b" HTTP/1.0\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: ",
			)
			.await?;
		socket.write_all(decimal(body.len(), &mut digits)).await?;
		socket.write_all(b"\r\n\r\n").await?;
		socket.write_all(body).await?;
		socket.flush().await
	};
	send.await.map_err(|_| Error::Report("send"))?;

	let mut got = 0;
	loop {
		if got == answer.len() {
			return Err(Error::Report("answer too long"));
		}
		match socket.read(&mut answer[got..]).await {
			Ok(0) => break,
			Ok(n) => got += n,
			Err(_) => return Err(Error::Report("no answer")),
		}
	}
	socket.close();

	// "HTTP/1.x NNN": the status starts at byte 9
	if got < 12 {
		return Err(Error::Report("no status"));
	}
	if answer[9] != b'2' {
		return Err(Error::Report("status not 2xx"));
	}
	let start = answer[..got]
		.windows(4)
		.position(|w| w == b"\r\n\r\n")
		.ok_or(Error::Report("no end of headers"))?
		+ 4;
	answer.copy_within(start..got, 0);
	Ok(got - start)
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
