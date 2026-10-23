//! Sniffer plumbing. The callback runs in the Wi-Fi driver's receive path: it
//! decodes with core, learns watched BSSIDs from beacons and probe responses,
//! and queues `Frame`s, and the capabilities of the requests that say how many
//! streams their sender can use.
//! No blocking, no allocation, no logging in there.

use core::{
	cell::Cell,
	sync::atomic::{AtomicU32, Ordering::Relaxed},
};

use chatter_core::models::{
	frame::{self, Capabilities, Frame, Mac},
	watched::{Watched, net_for_ssid},
};
use critical_section::Mutex;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use esp_radio::wifi::{
	WifiController,
	sniffer::{PromiscuousPkt, Sniffer},
};

use crate::{Result, config::WATCH_SSIDS};

// 14 B a frame; 64 overflowed during a LAN-wide port scan, when every device
// talks at once
const QUEUE_LEN: usize = 256;

static WATCHED: Mutex<Cell<Watched>> = Mutex::new(Cell::new(Watched::new()));
/// The sensor's own MAC: its reports aren't a device on the network.
static OWN: Mutex<Cell<Mac>> = Mutex::new(Cell::new(Mac([0; 6])));
static QUEUE: Channel<CriticalSectionRawMutex, Frame, QUEUE_LEN> = Channel::new();
static DROPPED: AtomicU32 = AtomicU32::new(0);

// requests are rare next to data frames, and drained after every frame
const CAPS_LEN: usize = 16;
static CAPS: Channel<CriticalSectionRawMutex, Capabilities, CAPS_LEN> = Channel::new();
static CAPS_DROPPED: AtomicU32 = AtomicU32::new(0);

/// Turns promiscuous mode on. Keep the returned `Sniffer` alive: dropping it
/// turns sniffing off again.
pub fn start(controller: &WifiController<'static>, watched: Watched, own: Mac) -> Result<Sniffer> {
	critical_section::with(|cs| {
		WATCHED.borrow(cs).set(watched);
		OWN.borrow(cs).set(own);
	});
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

pub fn try_capabilities() -> Option<Capabilities> {
	CAPS.try_receive().ok()
}

/// Frames lost because the queue was full, since the last call.
pub fn take_dropped() -> u32 {
	DROPPED.swap(0, Relaxed)
}

/// Requests lost because their queue was full, since the last call.
pub fn take_caps_dropped() -> u32 {
	CAPS_DROPPED.swap(0, Relaxed)
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
	let (watched, own) =
		critical_section::with(|cs| (WATCHED.borrow(cs).get(), OWN.borrow(cs).get()));
	if let Some(caps) = frame::capabilities(pkt.data)
		&& caps.streams() > 0
		&& CAPS.try_send(caps).is_err()
	{
		CAPS_DROPPED.fetch_add(1, Relaxed);
	}
	if let Some(frame) = frame::decode(pkt.data, len, rssi, &watched)
		&& frame.station != own
		&& QUEUE.try_send(frame).is_err()
	{
		DROPPED.fetch_add(1, Relaxed);
	}
}
