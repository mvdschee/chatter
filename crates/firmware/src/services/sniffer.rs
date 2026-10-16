//! Sniffer plumbing. The callback runs in the Wi-Fi driver's receive path: it
//! decodes with core, learns watched BSSIDs from beacons and probe responses,
//! and queues `Frame`s.
//! No blocking, no allocation, no logging in there.

use core::{
	cell::Cell,
	sync::atomic::{AtomicU32, Ordering::Relaxed},
};

use chatter_core::models::{
	frame::{self, Frame},
	watched::{Watched, net_for_ssid},
};
use critical_section::Mutex;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use esp_radio::wifi::{
	WifiController,
	sniffer::{PromiscuousPkt, Sniffer},
};

use crate::{Result, config::WATCH_SSIDS};

const QUEUE_LEN: usize = 64;

static WATCHED: Mutex<Cell<Watched>> = Mutex::new(Cell::new(Watched::new()));
static QUEUE: Channel<CriticalSectionRawMutex, Frame, QUEUE_LEN> = Channel::new();
static DROPPED: AtomicU32 = AtomicU32::new(0);

/// Turns promiscuous mode on. Keep the returned `Sniffer` alive: dropping it
/// turns sniffing off again.
pub fn start(controller: &WifiController<'static>, watched: Watched) -> Result<Sniffer> {
	critical_section::with(|cs| WATCHED.borrow(cs).set(watched));
	let mut sniffer = controller.sniffer();
	sniffer.set_receive_cb(on_frame);
	sniffer.set_promiscuous_mode(true)?;
	Ok(sniffer)
}

pub async fn next_frame() -> Frame {
	QUEUE.receive().await
}

pub fn watched() -> Watched {
	critical_section::with(|cs| WATCHED.borrow(cs).get())
}

/// Frames lost because the queue was full, since the last call.
pub fn take_dropped() -> u32 {
	DROPPED.swap(0, Relaxed)
}

fn on_frame(pkt: PromiscuousPkt<'_>) {
	if let Some((bssid, ssid)) = frame::announcement(pkt.data) {
		if let Some(net) = net_for_ssid(ssid, WATCH_SSIDS) {
			critical_section::with(|cs| {
				let cell = WATCHED.borrow(cs);
				let mut watched = cell.get();
				if watched.learn(net, bssid) {
					cell.set(watched);
				}
			});
		}
		return;
	}

	let len = pkt.len.min(u16::MAX as usize) as u16;
	let rssi = pkt.rx_cntl.rssi.clamp(i8::MIN as i32, i8::MAX as i32) as i8;
	if let Some(frame) = frame::decode(pkt.data, len, rssi, &watched())
		&& QUEUE.try_send(frame).is_err()
	{
		DROPPED.fetch_add(1, Relaxed);
	}
}
