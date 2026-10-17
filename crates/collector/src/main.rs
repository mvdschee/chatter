//! Development collector. Takes the sensor's reports on `POST /report`,
//! prints one line per report, and appends each one, stamped with the receive
//! time, to a JSON Lines file. With `--test-file-mib N` it also serves
//! `GET /test.bin` (N MiB of pseudo-random bytes) for the ESP32-C3 test client.
//!
//! ```text
//! chatter-collector <ip:port> [--out reports.jsonl] [--test-file-mib N]
//! ```
//!
//! Plain HTTP/1.0, one thread per connection, std only besides `serde_json`.
//! The real collector on the home server is plan step 10.

use std::{
	env,
	fs::OpenOptions,
	io::{BufRead, BufReader, Read, Write},
	net::{TcpListener, TcpStream},
	process::exit,
	sync::{Arc, Mutex},
	thread,
	time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

/// A report is at most 24 KiB (see core's `REPORT_BUF`); refuse anything much bigger.
const MAX_BODY: usize = 64 * 1024;

struct Config {
	addr: String,
	out: String,
	test_file_bytes: Option<u64>,
}

/// Shared between connections: the last `seq`, to spot gaps and restarts.
type LastSeq = Arc<Mutex<Option<u64>>>;

fn main() {
	let config = parse_args().unwrap_or_else(|e| {
		eprintln!(
			"{e}\nusage: chatter-collector <ip:port> [--out reports.jsonl] [--test-file-mib N]"
		);
		exit(2);
	});
	let listener = TcpListener::bind(&config.addr).unwrap_or_else(|e| {
		eprintln!("can't listen on {}: {e}", config.addr);
		exit(1);
	});
	println!("collector on {}, writing {}", config.addr, config.out);
	if let Some(bytes) = config.test_file_bytes {
		println!("serving GET /test.bin, {bytes} B");
	}

	let config = Arc::new(config);
	let last_seq: LastSeq = Arc::default();
	for stream in listener.incoming().flatten() {
		let (config, last_seq) = (config.clone(), last_seq.clone());
		thread::spawn(move || {
			if let Err(e) = handle(stream, &config, &last_seq) {
				eprintln!("connection: {e}");
			}
		});
	}
}

fn parse_args() -> Result<Config, String> {
	let mut args = env::args().skip(1);
	let addr = args.next().ok_or("missing <ip:port>")?;
	let mut config = Config {
		addr,
		out: "/tmp/chatter-reports.jsonl".into(),
		test_file_bytes: None,
	};
	while let Some(flag) = args.next() {
		let value = args.next().ok_or(format!("{flag} needs a value"))?;
		match flag.as_str() {
			"--out" => config.out = value,
			"--test-file-mib" => {
				let mib: u64 =
					value.parse().map_err(|_| format!("--test-file-mib: not a number: {value}"))?;
				config.test_file_bytes = Some(mib * 1024 * 1024);
			},
			_ => return Err(format!("unknown flag {flag}")),
		}
	}
	Ok(config)
}

fn handle(stream: TcpStream, config: &Config, last_seq: &LastSeq) -> Result<(), String> {
	let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
	let mut request_line = String::new();
	// a connection that closes without a request (the container healthcheck,
	// a port scan) gets no answer: writing one would only hit a closed socket
	if reader.read_line(&mut request_line).map_err(|e| e.to_string())? == 0 {
		return Ok(());
	}
	let mut parts = request_line.split_whitespace();
	let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));

	let mut content_length = 0;
	loop {
		let mut line = String::new();
		if reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 || line == "\r\n" {
			break;
		}
		if let Some((name, value)) = line.split_once(':')
			&& name.eq_ignore_ascii_case("content-length")
		{
			content_length = value.trim().parse().map_err(|_| "bad Content-Length")?;
		}
	}

	let mut stream = stream;
	match (method, path, config.test_file_bytes) {
		("POST", "/report", _) => {
			if content_length > MAX_BODY {
				respond(&mut stream, "413 Payload Too Large")?;
				return Err(format!("report too big: {content_length} B"));
			}
			let mut body = vec![0; content_length];
			reader.read_exact(&mut body).map_err(|e| e.to_string())?;
			match take_report(&body, config, last_seq) {
				Ok(()) => respond(&mut stream, "204 No Content"),
				Err(e) => {
					respond(&mut stream, "400 Bad Request")?;
					Err(e)
				},
			}
		},
		("GET", "/test.bin", Some(bytes)) => serve_test_file(&mut stream, bytes),
		_ => respond(&mut stream, "404 Not Found"),
	}
}

fn respond(stream: &mut TcpStream, status: &str) -> Result<(), String> {
	write!(stream, "HTTP/1.0 {status}\r\nContent-Length: 0\r\n\r\n").map_err(|e| e.to_string())
}

/// Prints one line for the report and appends it to the output file.
fn take_report(body: &[u8], config: &Config, last_seq: &LastSeq) -> Result<(), String> {
	let mut report: Value =
		serde_json::from_slice(body).map_err(|e| format!("bad JSON ({} B): {e}", body.len()))?;
	let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);

	let seq = report["seq"].as_u64().ok_or("report without seq")?;
	let gap = {
		let mut last = last_seq.lock().map_err(|_| "seq lock poisoned")?;
		let gap = match *last {
			Some(prev) if seq == prev + 1 => String::new(),
			Some(prev) if seq > prev => format!("  <- gap: expected {}", prev + 1),
			Some(_) => "  <- seq went back: sensor restarted".into(),
			None => String::new(),
		};
		*last = Some(seq);
		gap
	};

	let devices = report["devices"].as_array().map(Vec::as_slice).unwrap_or_default();
	let sum = |key: &str| devices.iter().filter_map(|d| d[key].as_u64()).sum::<u64>();
	println!(
		"{} seq {seq} uptime {}s ch {} nets {}: {} devices, rx {} B, tx {} B, dropped {}, overflow {}, {} B{gap}",
		utc_time(now),
		report["uptime_s"],
		report["channel"],
		report["nets_known"],
		devices.len(),
		sum("rx_bytes"),
		sum("tx_bytes"),
		report["dropped"],
		report["overflow"],
		body.len(),
	);

	if let Value::Object(fields) = &mut report {
		fields.insert("received_unix".into(), now.into());
	}
	let mut file = OpenOptions::new()
		.create(true)
		.append(true)
		.open(&config.out)
		.map_err(|e| e.to_string())?;
	writeln!(file, "{report}").map_err(|e| e.to_string())
}

/// `bytes` of xorshift output: not compressible, and the same every run.
fn serve_test_file(stream: &mut TcpStream, bytes: u64) -> Result<(), String> {
	write!(
		stream,
		"HTTP/1.0 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {bytes}\r\n\r\n"
	)
	.map_err(|e| e.to_string())?;
	let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
	let mut chunk = [0u8; 16 * 1024];
	let mut left = bytes;
	while left > 0 {
		for word in chunk.as_chunks_mut::<8>().0 {
			state ^= state << 13;
			state ^= state >> 7;
			state ^= state << 17;
			*word = state.to_le_bytes();
		}
		let n = left.min(chunk.len() as u64) as usize;
		stream.write_all(&chunk[..n]).map_err(|e| e.to_string())?;
		left -= n as u64;
	}
	println!("served /test.bin: {bytes} B");
	Ok(())
}

/// `HH:MM:SS` UTC; enough to line reports up with the logs.
fn utc_time(unix: u64) -> String {
	let day = unix % 86_400;
	format!("{:02}:{:02}:{:02}Z", day / 3600, day / 60 % 60, day % 60)
}
