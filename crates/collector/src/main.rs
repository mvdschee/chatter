//! Collector: takes the sensors' reports on `POST /report` and stores them in
//! SQLite, one row per report and per device minute. Serves the monitoring
//! page on `/` and its data on `/v1/overview`. Settings come from the
//! environment, see `config.rs`.
//!
//! `chatter-collector port-scan` runs one port scan now, and exits.

pub use error::{Error, Result};

mod config;
mod core;
mod error;
mod repository;
mod services;
mod utils;

use crate::{
	core::{
		models::{
			config::{Command, Config},
			identify::SharedIdentify,
		},
		setup::{
			HandleBaselines, HandleDatabase, HandleIdentify, HandlePortScan, HandleScans,
			HandleServerApi, HandleVendors,
		},
	},
	services::web::HttpServerState,
};

#[tokio::main]
async fn main() -> Result<()> {
	let config = Config::init()?;

	let db = HandleDatabase::new(&config).run().await?;

	let vendors = HandleVendors::new(&config).run();

	if config.command == Command::PortScan {
		return HandlePortScan::new(db, &config, vendors).run().await;
	}

	HandleScans::new(db.clone(), &config, vendors).run();
	HandleBaselines::new(db.clone()).run();
	let identify = SharedIdentify::default();
	HandleIdentify::new(db.clone(), &config, identify.clone()).run();

	let state = HttpServerState {
		db,
		ui_dir: config.ui_dir.clone(),
		data_dir: config.data_dir(),
		identify,
	};
	HandleServerApi::new(&config, state).await?.run().await?;

	Err(Error::MainLoopClosed)
}
