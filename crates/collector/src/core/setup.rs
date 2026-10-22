use std::{
	path::PathBuf,
	sync::{Arc, RwLock},
};

use crate::{
	Result,
	core::{
		handlers::{
			baseline::baseline_loop,
			scan::{current_targets, port_scan, port_scan_loop, sweep_loop},
			vendor::{load_vendors, vendor_loop},
		},
		models::{config::Config, vendor::SharedVendors},
	},
	info,
	repository::db::DatabaseRepository,
	services::web::{HttpServer, HttpServerConfig, HttpServerState},
};

/// Opens the database and brings its tables up to date.
pub struct HandleDatabase {
	path: String,
}

impl HandleDatabase {
	pub fn new(config: &Config) -> Self {
		Self {
			path: config.db_path.clone(),
		}
	}

	pub async fn run(self) -> Result<DatabaseRepository> {
		let db = DatabaseRepository::new(&self.path).await?;
		db.init().await?;
		Ok(db)
	}
}

/// The HTTP server: the sensors' reports, and the page and its data.
pub struct HandleServerApi {
	server: HttpServer,
}

impl HandleServerApi {
	pub async fn new(config: &Config, state: HttpServerState) -> Result<Self> {
		let server = HttpServer::new(
			HttpServerConfig {
				bind: &config.bind,
			},
			state,
		)
		.await?;
		Ok(Self {
			server,
		})
	}

	pub async fn run(self) -> Result<()> {
		self.server.run().await
	}
}

/// The network scans in the background.
pub struct HandleScans {
	db: DatabaseRepository,
	config: Config,
	vendors: SharedVendors,
}

impl HandleScans {
	pub fn new(db: DatabaseRepository, config: &Config, vendors: SharedVendors) -> Self {
		Self {
			db,
			config: config.clone(),
			vendors,
		}
	}

	pub fn run(self) {
		let every = std::time::Duration::from_secs(self.config.sweep_every_min * 60);
		match &self.config.scan_targets {
			Some(targets) => info!("sweeping {targets} every {} min", self.config.sweep_every_min),
			None => {
				info!("sweeping this machine's network every {} min", self.config.sweep_every_min)
			},
		}
		tokio::spawn(sweep_loop(
			self.db.clone(),
			self.config.scan_targets.clone(),
			every,
			self.vendors.clone(),
		));
		match self.config.port_scan_at {
			Some(at) => {
				info!("port scan daily at {at}");
				tokio::spawn(port_scan_loop(self.db, self.config.scan_targets, at, self.vendors));
			},
			None => info!("CHATTER_PORT_SCAN_AT=off: no port scans"),
		}
	}
}

/// Each device's usual traffic, worked out again every hour.
pub struct HandleBaselines {
	db: DatabaseRepository,
}

impl HandleBaselines {
	pub fn new(db: DatabaseRepository) -> Self {
		Self {
			db,
		}
	}

	pub fn run(self) {
		tokio::spawn(baseline_loop(self.db));
	}
}

/// One port scan, now (`chatter-collector port-scan`).
pub struct HandlePortScan {
	db: DatabaseRepository,
	config: Config,
	vendors: SharedVendors,
}

impl HandlePortScan {
	pub fn new(db: DatabaseRepository, config: &Config, vendors: SharedVendors) -> Self {
		Self {
			db,
			config: config.clone(),
			vendors,
		}
	}

	pub async fn run(self) -> Result<()> {
		let targets = current_targets(self.config.scan_targets.as_ref())?;
		info!("port scan of {targets}: this takes about half an hour");
		let (hosts, ports, timed_out) = port_scan(&self.db, &targets, &self.vendors).await?;
		info!("port scan of {targets}: {hosts} hosts, {ports} open ports, {timed_out} timed out");
		Ok(())
	}
}

/// IEEE's vendor list: the copy in the data directory now, and a refresh in
/// the background when it's missing or a month old.
pub struct HandleVendors {
	path: PathBuf,
}

impl HandleVendors {
	pub fn new(config: &Config) -> Self {
		Self {
			path: config.data_dir().join("oui.csv"),
		}
	}

	pub fn run(self) -> SharedVendors {
		let vendors: SharedVendors = Arc::new(RwLock::new(load_vendors(&self.path)));
		tokio::spawn(vendor_loop(self.path, vendors.clone()));
		vendors
	}
}
