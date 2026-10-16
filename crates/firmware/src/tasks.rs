//! Loops that drive core. They own the timing and the I/O; core does the
//! counting.

use core::net::SocketAddrV4;

use chatter_core::{
	handlers::aggregate,
	models::{
		counters::{Device, DeviceTable, format_bytes},
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

// ~5 KB: in static memory, so it never sits on the stack
static TABLE: ConstStaticCell<DeviceTable> = ConstStaticCell::new(DeviceTable::new());

/// The report waiting to be sent. The aggregate task writes it and the report
/// task sends it, so a slow collector never stalls the counting.
struct Pending {
	seq: u32,
	len: usize,
	json: [u8; REPORT_BUF],
}
static PENDING: Mutex<CriticalSectionRawMutex, Pending> = Mutex::new(Pending {
	seq: 0,
	len: 0,
	json: [0; REPORT_BUF],
});
static READY: Signal<CriticalSectionRawMutex, ()> = Signal::new();

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
	let mut known = Watched::new();
	let mut seq: u32 = 0;
	loop {
		let deadline = Instant::now() + Duration::from_secs(PERIOD_SECS);
		while let Ok(frame) = with_deadline(deadline, sniffer::next_frame()).await {
			aggregate::record(table, &frame);
		}
		log_learned(&mut known);
		finish_period(table, seq);
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

/// Logs the period, writes it as the pending report, and empties the table.
fn finish_period(table: &mut DeviceTable, seq: u32) {
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

	match PENDING.try_lock() {
		Ok(mut pending) => match write_report(&mut pending.json, &header, logged, &broadcast) {
			Ok(len) => {
				pending.seq = seq;
				pending.len = len;
				READY.signal(());
			},
			Err(_) => warn!("report {}: didn't fit the buffer", seq),
		},
		// still sending the last one: skip this report, the gap shows in `seq`
		Err(_) => {
			logged.for_each(drop);
			warn!("report {}: skipped, the previous one is still sending", seq);
		},
	}
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

/// Sends each pending report to the collector.
#[embassy_executor::task]
pub async fn send_reports(stack: Stack<'static>) {
	let addr: SocketAddrV4 =
		COLLECTOR_ADDR.parse().expect("CHATTER_COLLECTOR_ADDR is not <ip>:<port>");
	let rx = SOCKET_RX.take();
	let tx = SOCKET_TX.take();
	loop {
		READY.wait().await;
		stack.wait_config_up().await;
		let pending = PENDING.lock().await;
		// reports written while we waited all landed in `pending`; this sends the
		// newest, so a signal raised meanwhile must not send it a second time
		READY.reset();
		let body = &pending.json[..pending.len];
		for attempt in 1..=SEND_ATTEMPTS {
			match report::post(stack, addr, body, rx, tx).await {
				Ok(()) => {
					info!("report {}: sent, {} B (attempt {})", pending.seq, pending.len, attempt);
					break;
				},
				Err(e) if attempt < SEND_ATTEMPTS => {
					warn!("report {}: attempt {} failed: {}, retrying", pending.seq, attempt, e);
					Timer::after(Duration::from_secs(RETRY_DELAY_SECS)).await;
				},
				Err(e) => warn!("report {}: not sent: {}", pending.seq, e),
			}
		}
	}
}
