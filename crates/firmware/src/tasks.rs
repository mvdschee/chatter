//! Loops that drive core. They own the timing and the I/O; core does the
//! counting.

use core::net::SocketAddrV4;

use chatter_core::{
	handlers::aggregate,
	models::{
		counters::{Device, DeviceTable, format_bytes},
		outbox::Outbox,
		report::{REPORT_BUF, ReportHeader, write_report},
		watched::{MAX_NETS, Watched},
	},
};
use defmt::{info, warn};
use embassy_net::Stack;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex, signal::Signal};
use embassy_time::{Duration, Instant, Timer, with_deadline};
use esp_radio::wifi::sniffer::Sniffer;
use static_cell::ConstStaticCell;

use crate::{
	config::{COLLECTOR_ADDR, PERIOD_SECS, SENSOR_NAME},
	services::{report, sniffer, wifi},
};

const SEND_ATTEMPTS: u32 = 2;
const RETRY_DELAY_SECS: u64 = 2;
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

static SOCKET_RX: ConstStaticCell<[u8; 256]> = ConstStaticCell::new([0; 256]);
// a whole report fits, so writing it never waits for ACKs halfway
static SOCKET_TX: ConstStaticCell<[u8; REPORT_BUF]> = ConstStaticCell::new([0; REPORT_BUF]);

/// Formats bytes for logging; `from_utf8` does not allocate.
fn fmt(bytes: u32, buf: &mut [u8; 16]) -> &str {
	let len = format_bytes(bytes, buf);
	// always ASCII; the fallback just avoids a panic path
	core::str::from_utf8(&buf[..len]).unwrap_or("?")
}

/// Counts queued frames per device, and every period hands a report to the
/// report task. Owns the `Sniffer`, because dropping it turns sniffing off.
#[embassy_executor::task]
pub async fn aggregate(sniffer: Sniffer) {
	let _sniffer = sniffer;
	let table = TABLE.take();
	let scratch = SCRATCH.take();
	let mut known = Watched::new();
	let mut seq: u32 = 0;
	loop {
		let deadline = Instant::now() + Duration::from_secs(PERIOD_SECS);
		while let Ok(frame) = with_deadline(deadline, sniffer::next_frame()).await {
			aggregate::record(table, &frame);
		}
		log_learned(&mut known);
		finish_period(table, scratch, seq).await;
		seq = seq.wrapping_add(1);
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

/// Sends the outbox to the collector, oldest first. A report only leaves the
/// outbox once the collector has answered; after a failure the rest waits for
/// the next period.
#[embassy_executor::task]
pub async fn send_reports(stack: Stack<'static>) {
	let addr: SocketAddrV4 =
		COLLECTOR_ADDR.parse().expect("CHATTER_COLLECTOR_ADDR is not <ip>:<port>");
	let rx = SOCKET_RX.take();
	let tx = SOCKET_TX.take();
	let sending = SENDING.take();
	loop {
		READY.wait().await;
		stack.wait_config_up().await;
		loop {
			let next = {
				let outbox = OUTBOX.lock().await;
				outbox.front().map(|(seq, report)| {
					sending[..report.len()].copy_from_slice(report);
					(seq, report.len(), outbox.len())
				})
			};
			let Some((seq, len, waiting)) = next else {
				break;
			};
			if !send(stack, addr, seq, &sending[..len], rx, tx).await {
				break;
			}
			info!("report {}: sent, {} B ({} were waiting)", seq, len, waiting);
			let mut outbox = OUTBOX.lock().await;
			if outbox.front().map(|(front, _)| front) == Some(seq) {
				outbox.pop_front();
			}
		}
	}
}

/// Posts one report, with one retry. `true` once the collector has it.
async fn send(
	stack: Stack<'static>,
	addr: SocketAddrV4,
	seq: u32,
	body: &[u8],
	rx: &mut [u8],
	tx: &mut [u8],
) -> bool {
	for attempt in 1..=SEND_ATTEMPTS {
		match report::post(stack, addr, body, rx, tx).await {
			Ok(()) => return true,
			Err(e) if attempt < SEND_ATTEMPTS => {
				warn!("report {}: attempt {} failed: {}, retrying", seq, attempt, e);
				Timer::after(Duration::from_secs(RETRY_DELAY_SECS)).await;
			},
			Err(e) => warn!("report {}: not sent, stays in the outbox: {}", seq, e),
		}
	}
	false
}
