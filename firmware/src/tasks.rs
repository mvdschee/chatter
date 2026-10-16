//! Loops that drive core. They own the timing and the I/O; core does the
//! counting.

use chatter_core::{
	handlers::aggregate,
	models::{
		counters::{DeviceTable, format_bytes},
		watched::{MAX_NETS, Watched},
	},
};
use defmt::info;
use embassy_time::{Duration, Instant, with_deadline};
use esp_radio::wifi::sniffer::Sniffer;
use static_cell::ConstStaticCell;

use crate::{config::PERIOD_SECS, services::sniffer};

// ~5 KB: in static memory, so it never sits on the stack
static TABLE: ConstStaticCell<DeviceTable> = ConstStaticCell::new(DeviceTable::new());

/// Formats bytes for logging; `from_utf8` does not allocate.
fn fmt(bytes: u32, buf: &mut [u8; 16]) -> &str {
	let len = format_bytes(bytes, buf);
	// always ASCII; the fallback just avoids a panic path
	core::str::from_utf8(&buf[..len]).unwrap_or("?")
}

/// Counts queued frames per device and logs the table every period. Owns the
/// `Sniffer`, because dropping it turns sniffing off.
#[embassy_executor::task]
pub async fn aggregate(sniffer: Sniffer) {
	let _sniffer = sniffer;
	let table = TABLE.take();
	let mut known = Watched::new();
	loop {
		let deadline = Instant::now() + Duration::from_secs(PERIOD_SECS);
		while let Ok(frame) = with_deadline(deadline, sniffer::next_frame()).await {
			aggregate::record(table, &frame);
		}
		log_learned(&mut known);
		log_period(table);
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

fn log_period(table: &mut DeviceTable) {
	// The buffers just need to outlive the log line; NLL drops the borrows after it.
	let mut tx = [0u8; 16];
	let mut rx = [0u8; 16];
	let mut devices = 0;
	for d in aggregate::drain(table) {
		devices += 1;
		let c = d.counters;
		info!(
			"dev {=[u8]:02x} net {}: tx {} frames {}, rx {} frames {}, retries {}, rssi {}",
			d.mac.0,
			d.net,
			c.tx_frames,
			fmt(c.tx_bytes, &mut tx),
			c.rx_frames,
			fmt(c.rx_bytes, &mut rx),
			c.retries,
			c.rssi_max
		);
	}
	let (broadcast, overflow) = aggregate::take_totals(table);
	for (net, b) in broadcast.iter().enumerate().filter(|(_, b)| b.rx_frames > 0) {
		info!("broadcast net {}: {} frames {}", net, b.rx_frames, fmt(b.rx_bytes, &mut rx));
	}
	let (beacons, watched_beacons) = sniffer::take_beacons();
	info!(
		"period {}s: {} devices, overflow {}, dropped {}, beacons {} ({} watched)",
		PERIOD_SECS,
		devices,
		overflow,
		sniffer::take_dropped(),
		beacons,
		watched_beacons
	);
}
