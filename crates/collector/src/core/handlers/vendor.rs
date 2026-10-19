use std::{path::Path, time::Duration};

use crate::{
	Result,
	core::models::{
		scan::Scan,
		vendor::{SharedVendors, Vendors},
	},
	info,
	services::{filesystem, http},
	warn,
};

/// IEEE's MA-L registry: every vendor's MAC prefixes.
const OUI_URL: &str = "https://standards-oui.ieee.org/oui/oui.csv";
/// It changes slowly; a month old is fresh enough.
const MAX_AGE: Duration = Duration::from_secs(30 * 86_400);
/// How often the loop checks the copy's age.
const CHECK_EVERY: Duration = Duration::from_secs(86_400);

/// The vendor list from the copy at `path`; empty without one (the scans then
/// keep nmap's vendor names).
pub fn load_vendors(path: &Path) -> Vendors {
	if !path.exists() {
		info!("no vendor list yet: nmap's names until it's downloaded");
		return Vendors::default();
	}
	match filesystem::read_to_string(&path.to_string_lossy()).and_then(|t| Vendors::from_csv(&t)) {
		Ok(v) => v,
		Err(e) => {
			warn!("vendor list: {e}; using nmap's");
			Vendors::default()
		},
	}
}

/// Downloads the list when the copy is missing or older than a month, and
/// swaps it in. A failed download keeps the old copy.
async fn refresh(path: &Path, shared: &SharedVendors) -> Result<()> {
	if filesystem::age(path).is_some_and(|age| age < MAX_AGE) {
		return Ok(());
	}
	http::download(&http::create_client()?, OUI_URL, path).await?;
	let vendors = load_vendors(path);
	info!("vendor list: {} prefixes from IEEE", vendors.len());
	if let Ok(mut current) = shared.write() {
		*current = vendors;
	}
	Ok(())
}

/// Refreshes now if needed, then checks daily.
pub async fn vendor_loop(path: std::path::PathBuf, shared: SharedVendors) {
	loop {
		if let Err(e) = refresh(&path, &shared).await {
			warn!("vendor list: {e}; keeping the copy there is");
		}
		tokio::time::sleep(CHECK_EVERY).await;
	}
}

/// IEEE's name for each host's vendor, else nmap's.
pub fn apply_vendors(scan: &mut Scan, vendors: &SharedVendors) {
	let Ok(vendors) = vendors.read() else {
		return;
	};
	for host in &mut scan.hosts {
		if let Some(name) = vendors.lookup(&host.mac) {
			host.vendor = Some(name.to_string());
		}
	}
}
