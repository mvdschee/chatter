//! Loops that drive core. They own the timing and the I/O; core does the
//! counting.
//!
//! A boot first asks the collector what to do, then counts until the
//! collector says to identify, identifies until it says to stop or the time
//! is up, and then waits for a reboot. Identifying never goes back to
//! counting: the sensor has been carried away from its place.

use core::{cell::Cell, net::SocketAddrV4};

use chatter_core::{
	handlers::{aggregate, identify},
	models::{
		answer::{Answer, parse_answer},
		counters::{Device, DeviceTable, format_bytes},
		identify::{IDENTIFY_BUF, IdentifyTable, write_identify},
		outbox::Outbox,
		report::{REPORT_BUF, ReportHeader, write_report},
		watched::{MAX_NETS, Watched},
	},
};
use defmt::{info, warn};
use embassy_net::Stack;
use embassy_sync::{
	blocking_mutex::{self, raw::CriticalSectionRawMutex},
	mutex::Mutex,
	signal::Signal,
};
use embassy_time::{Duration, Instant, Timer, with_deadline, with_timeout};
use esp_radio::wifi::sniffer::Sniffer;
use static_cell::ConstStaticCell;

use crate::{
	Error,
	config::{COLLECTOR_ADDR, IDENTIFY_SECS, MODE_PATH, PERIOD_SECS, SENSOR_NAME},
	services::{
		report::{self, Collector},
		sniffer, wifi,
	},
};

const SEND_ATTEMPTS: u32 = 2;
const RETRY_DELAY_SECS: u64 = 2;
/// How long a boot waits for an address before it counts without asking.
const BOOT_NET_SECS: u64 = 30;
/// A request gives up after this long. The socket's own timeout only covers
/// data waiting for an ACK, not an answer that never comes.
const REQUEST_SECS: u64 = 20;
/// Fits the collector's answer, headers included.
const ANSWER_BUF: usize = 256;
/// About 10 typical reports, or one worst-case report.
const OUTBOX_BYTES: usize = 32 * 1024;
const OUTBOX_MAX: usize = 16;

// ~5 KB: in static memory, so it never sits on the stack
static TABLE: ConstStaticCell<DeviceTable> = ConstStaticCell::new(DeviceTable::new());

/// Reports waiting to be sent, oldest first. The aggregate task adds one every
/// period and the report task removes it once the collector has it, so a slow
/// or missing collector costs nothing until the outbox overflows.
static OUTBOX: Mutex<CriticalSectionRawMutex, Outbox<OUTBOX_BYTES, OUTBOX_MAX>> =
	Mutex::new(Outbox::new());
static READY: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// The aggregate task writes each report here before it goes into the outbox.
static SCRATCH: ConstStaticCell<[u8; REPORT_BUF]> = ConstStaticCell::new([0; REPORT_BUF]);
/// The report task's copy of the report it's sending, so the outbox stays
/// unlocked during network I/O.
static SENDING: ConstStaticCell<[u8; REPORT_BUF]> = ConstStaticCell::new([0; REPORT_BUF]);

static IDENTIFY_TABLE: ConstStaticCell<IdentifyTable> = ConstStaticCell::new(IdentifyTable::new());

/// Until when the sensor identifies; `None` while it counts. Set from the
/// collector's answers by the report task.
static IDENTIFY_UNTIL: blocking_mutex::Mutex<CriticalSectionRawMutex, Cell<Option<Instant>>> =
	blocking_mutex::Mutex::new(Cell::new(None));
/// Set once the report task has asked the collector what to do at boot.
static STARTED: Signal<CriticalSectionRawMutex, ()> = Signal::new();
/// The newest identify post and its length, for the report task; the length
/// is 0 once it's sent.
static IDENTIFY_POST: Mutex<CriticalSectionRawMutex, ([u8; IDENTIFY_BUF], usize)> =
	Mutex::new(([0; IDENTIFY_BUF], 0));

static SOCKET_RX: ConstStaticCell<[u8; 256]> = ConstStaticCell::new([0; 256]);
// a whole report fits, so writing it never waits for ACKs halfway
static SOCKET_TX: ConstStaticCell<[u8; REPORT_BUF]> = ConstStaticCell::new([0; REPORT_BUF]);

/// Formats bytes for logging; `from_utf8` does not allocate.
fn fmt(bytes: u32, buf: &mut [u8; 16]) -> &str {
	let len = format_bytes(bytes, buf);
	// always ASCII; the fallback just avoids a panic path
	core::str::from_utf8(&buf[..len]).unwrap_or("?")
}

fn identifying() -> bool {
	IDENTIFY_UNTIL.lock(Cell::get).is_some_and(|until| Instant::now() < until)
}

/// Sets the mode from the collector's answer.
fn apply(answer: Answer) {
	let until = match answer {
		Answer::Identify {
			secs,
		} => Some(Instant::now() + Duration::from_secs(secs.into())),
		Answer::Count => None,
		Answer::CarryOn => return,
	};
	match (IDENTIFY_UNTIL.lock(|u| u.replace(until)), until) {
		(None, Some(_)) => info!("collector: identify"),
		(Some(_), None) => info!("collector: stop identifying"),
		_ => {},
	}
}

/// Counts or identifies, as the collector says, handing each report or post
/// to the report task. Owns the `Sniffer`, because dropping it turns
/// sniffing off.
#[embassy_executor::task]
pub async fn aggregate(sniffer: Sniffer) {
	let _sniffer = sniffer;
	STARTED.wait().await;
	// frames lost while waiting aren't any period's
	sniffer::take_dropped();
	if !identifying() {
		count_minutes(TABLE.take(), SCRATCH.take()).await;
		info!("counting stopped");
	}
	identify_devices(IDENTIFY_TABLE.take()).await;
	info!("identify over: waiting for a reboot");
	core::future::pending::<()>().await
}

/// Counts frames per device and adds a report to the outbox every period,
/// until the collector says to identify.
async fn count_minutes(table: &mut DeviceTable, scratch: &mut [u8; REPORT_BUF]) {
	let mut known = Watched::new();
	let mut seq: u32 = 0;
	loop {
		let deadline = Instant::now() + Duration::from_secs(PERIOD_SECS);
		while let Ok(frame) = with_deadline(deadline, sniffer::next_frame()).await {
			if identifying() {
				break;
			}
			aggregate::record(table, &frame);
		}
		if identifying() {
			// a part of a minute would be stored as a whole one
			aggregate::drain(table).for_each(drop);
			aggregate::take_totals(table);
			return;
		}
		log_learned(&mut known);
		finish_period(table, scratch, seq).await;
		seq = seq.wrapping_add(1);
	}
}

/// Every few seconds, hands the devices heard to the report task, strongest
/// first, until the session ends.
async fn identify_devices(table: &mut IdentifyTable) {
	while identifying() {
		let deadline = Instant::now() + Duration::from_secs(IDENTIFY_SECS);
		while let Ok(frame) = with_deadline(deadline, sniffer::next_frame()).await {
			identify::record(table, &frame);
		}
		let mut sent = 0;
		let mut strongest = None;
		let heard = identify::drain_strongest(table).inspect(|h| {
			sent += 1;
			strongest.get_or_insert(*h);
		});
		let mut post = IDENTIFY_POST.lock().await;
		let (buf, len) = &mut *post;
		match write_identify(buf, SENSOR_NAME, heard) {
			Ok(n) => *len = n,
			Err(_) => {
				*len = 0;
				warn!("identify: post didn't fit the buffer");
			},
		}
		drop(post);
		READY.signal(());
		match strongest {
			Some(h) => info!(
				"identify: {} devices, strongest {=[u8]:02x} at {}",
				sent, h.mac.0, h.rssi_max
			),
			None => info!("identify: nothing heard"),
		}
	}
}

fn log_learned(known: &mut Watched) {
	let now = sniffer::watched();
	for net in 0..MAX_NETS as u8 {
		if let Some(bssid) = now.bssid(net)
			&& known.learn(net, bssid)
		{
			info!("watch: net {} is bssid {=[u8]:02x}", net, bssid.0);
		}
	}
}

/// Logs the period, adds its report to the outbox, and empties the table.
async fn finish_period(table: &mut DeviceTable, scratch: &mut [u8; REPORT_BUF], seq: u32) {
	let (broadcast, overflow) = aggregate::take_totals(table);
	let header = ReportHeader {
		sensor: SENSOR_NAME,
		seq,
		uptime_s: Instant::now().as_secs(),
		channel: wifi::channel(),
		dropped: sniffer::take_dropped(),
		overflow,
		nets_known: core::array::from_fn(|net| sniffer::watched().bssid(net as u8).is_some()),
	};
	let mut devices = 0;
	let logged = aggregate::drain(table).inspect(|d| {
		devices += 1;
		log_device(d);
	});

	match write_report(scratch, &header, logged, &broadcast) {
		// the report task only holds the lock to copy or remove one report
		Ok(len) => match OUTBOX.lock().await.push(seq, &scratch[..len]) {
			Ok(0) => {},
			Ok(dropped) => warn!("outbox full: dropped the {} oldest report(s)", dropped),
			Err(_) => warn!("report {}: bigger than the outbox", seq),
		},
		Err(_) => warn!("report {}: didn't fit the buffer", seq),
	}
	READY.signal(());
	info!(
		"period {}: {} devices, overflow {}, dropped {}",
		seq, devices, header.overflow, header.dropped
	);
}

fn log_device(d: &Device) {
	let mut tx = [0u8; 16];
	let mut rx = [0u8; 16];
	let c = d.counters;
	info!(
		"dev {=[u8]:02x} net {}: tx {} frames {} ({} B), rx {} frames {} ({} B), retries {}, rssi {}",
		d.mac.0,
		d.net,
		c.tx_frames,
		fmt(c.tx_bytes, &mut tx),
		c.tx_bytes,
		c.rx_frames,
		fmt(c.rx_bytes, &mut rx),
		c.rx_bytes,
		c.retries,
		c.rssi_max
	);
}

/// Asks the collector what to do at boot, then sends the outbox and the
/// identify posts as they come. A report only leaves the outbox once the
/// collector has answered; after a failure the rest waits for the next
/// period. An identify post that fails is dropped: a newer one follows.
#[embassy_executor::task]
pub async fn send_reports(stack: Stack<'static>) {
	let addr: SocketAddrV4 =
		COLLECTOR_ADDR.parse().expect("CHATTER_COLLECTOR_ADDR is not <ip>:<port>");
	let mut collector = Collector {
		stack,
		addr,
		rx: SOCKET_RX.take(),
		tx: SOCKET_TX.take(),
	};
	let sending = SENDING.take();

	// without an answer the sensor counts
	if with_timeout(Duration::from_secs(BOOT_NET_SECS), stack.wait_config_up()).await.is_ok() {
		send(&mut collector, "GET", MODE_PATH, &[]).await;
	}
	STARTED.signal(());

	loop {
		READY.wait().await;
		stack.wait_config_up().await;
		send_outbox(&mut collector, sending).await;
		send_identify(&mut collector, sending).await;
	}
}

async fn send_outbox(collector: &mut Collector<'_>, sending: &mut [u8; REPORT_BUF]) {
	loop {
		let next = {
			let outbox = OUTBOX.lock().await;
			outbox.front().map(|(seq, report)| {
				sending[..report.len()].copy_from_slice(report);
				(seq, report.len(), outbox.len())
			})
		};
		let Some((seq, len, waiting)) = next else {
			return;
		};
		if !send(collector, "POST", "/report", &sending[..len]).await {
			warn!("report {}: stays in the outbox", seq);
			return;
		}
		info!("report {}: sent, {} B ({} were waiting)", seq, len, waiting);
		let mut outbox = OUTBOX.lock().await;
		if outbox.front().map(|(front, _)| front) == Some(seq) {
			outbox.pop_front();
		}
	}
}

async fn send_identify(collector: &mut Collector<'_>, sending: &mut [u8; REPORT_BUF]) {
	let len = {
		let mut post = IDENTIFY_POST.lock().await;
		let (buf, len) = &mut *post;
		sending[..*len].copy_from_slice(&buf[..*len]);
		core::mem::take(len)
	};
	if len > 0 {
		send(collector, "POST", "/identify", &sending[..len]).await;
	}
}

/// Sends one request, with one retry, and applies the collector's answer.
/// `true` once the collector has it.
async fn send(collector: &mut Collector<'_>, method: &str, path: &str, body: &[u8]) -> bool {
	let mut answer = [0u8; ANSWER_BUF];
	for attempt in 1..=SEND_ATTEMPTS {
		let request = report::request(collector, method, path, body, &mut answer);
		let result = with_timeout(Duration::from_secs(REQUEST_SECS), request)
			.await
			.unwrap_or(Err(Error::Report("timed out")));
		match result {
			Ok(len) => {
				apply(parse_answer(&answer[..len]));
				return true;
			},
			Err(e) if attempt < SEND_ATTEMPTS => {
				warn!("{} {}: attempt {} failed: {}, retrying", method, path, attempt, e);
				Timer::after(Duration::from_secs(RETRY_DELAY_SECS)).await;
			},
			Err(e) => warn!("{} {}: not sent: {}", method, path, e),
		}
	}
	false
}
