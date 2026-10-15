//! Sniffer spike: counts what the radio hears while the station is connected.
//! The callback runs in the Wi-Fi driver's receive path, so it only touches
//! atomics: no blocking, no allocation, no logging.

use core::{
	cell::Cell,
	sync::atomic::{AtomicU32, Ordering::Relaxed},
};

use critical_section::Mutex;
use defmt::info;
use embassy_time::{Duration, Timer};
use esp_radio::wifi::{
	WifiController,
	sniffer::{PromiscuousPkt, Sniffer},
};

use crate::Result;

// `wifi_promiscuous_pkt_type_t` values; esp-radio doesn't re-export them
const MGMT: usize = 0;
const DATA: usize = 2;
const MISC: usize = 3;

// frame control (2) + duration (2), then addr1, addr2, addr3
const ADDR_OFFSETS: [usize; 3] = [4, 10, 16];
const HEADER_LEN: usize = 24;

const REPORT_SECS: u64 = 10;

static BSSID: Mutex<Cell<[u8; 6]>> = Mutex::new(Cell::new([0; 6]));
static FRAMES: [AtomicU32; 4] = [const { AtomicU32::new(0) }; 4];
static BYTES: AtomicU32 = AtomicU32::new(0);
static OURS_FRAMES: AtomicU32 = AtomicU32::new(0);
static OURS_DATA: AtomicU32 = AtomicU32::new(0);
static OURS_BYTES: AtomicU32 = AtomicU32::new(0);
static SHORT: AtomicU32 = AtomicU32::new(0);

/// Turns promiscuous mode on. Keep the returned `Sniffer` alive: dropping it
/// turns sniffing off again.
pub fn start(controller: &WifiController<'static>, bssid: [u8; 6]) -> Result<Sniffer> {
	critical_section::with(|cs| BSSID.borrow(cs).set(bssid));
	let mut sniffer = controller.sniffer();
	sniffer.set_receive_cb(on_frame);
	sniffer.set_promiscuous_mode(true)?;
	Ok(sniffer)
}

fn on_frame(pkt: PromiscuousPkt<'_>) {
	FRAMES[(pkt.frame_type as usize).min(MISC)].fetch_add(1, Relaxed);
	BYTES.fetch_add(pkt.len as u32, Relaxed);

	// MIMO frames arrive as a length only, without a header
	if pkt.data.len() < HEADER_LEN {
		SHORT.fetch_add(1, Relaxed);
		return;
	}

	let bssid = critical_section::with(|cs| BSSID.borrow(cs).get());
	let ours = ADDR_OFFSETS.iter().any(|&at| pkt.data[at..at + 6] == bssid);
	if ours {
		OURS_FRAMES.fetch_add(1, Relaxed);
		OURS_BYTES.fetch_add(pkt.len as u32, Relaxed);
		if pkt.frame_type as usize == DATA {
			OURS_DATA.fetch_add(1, Relaxed);
		}
	}
}

/// Logs and resets the counters every 10 s. Owns the `Sniffer` to keep it on.
#[embassy_executor::task]
pub async fn report(sniffer: Sniffer) {
	let _sniffer = sniffer;
	loop {
		Timer::after(Duration::from_secs(REPORT_SECS)).await;
		info!(
			"sniff {}s: mgmt {} data {} misc {}, {} bytes | ours {} frames ({} data), {} bytes | short {}",
			REPORT_SECS,
			FRAMES[MGMT].swap(0, Relaxed),
			FRAMES[DATA].swap(0, Relaxed),
			FRAMES[MISC].swap(0, Relaxed),
			BYTES.swap(0, Relaxed),
			OURS_FRAMES.swap(0, Relaxed),
			OURS_DATA.swap(0, Relaxed),
			OURS_BYTES.swap(0, Relaxed),
			SHORT.swap(0, Relaxed),
		);
	}
}
