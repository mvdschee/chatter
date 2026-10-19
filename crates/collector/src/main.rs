//! Collector: takes the sensors' reports on `POST /report` and stores them in
//! SQLite, one row per report and per device minute. Serves the monitoring
//! page on `/` and its data on `/v1/overview`. With
//! `CHATTER_TEST_FILE_MIB` set it also serves `GET /test.bin` for the ESP32-C3
//! test client. Settings come from the environment, see `config.rs`.
//!
//! `chatter-collector import <reports.jsonl>` loads the first collector's file
//! of reports instead, and exits; `chatter-collector port-scan` runs one port
//! scan now, and exits.

pub use error::{Error, Result};

mod config;
mod core;
mod error;
mod repository;
mod services;
mod utils;

use crate::{
	core::{
		models::config::{Command, Config},
		setup::{
			HandleBaselines, HandleDatabase, HandleImport, HandlePortScan, HandleScans,
			HandleServerApi, HandleVendors,
		},
	},
	services::web::{HttpServerState, handlers::test_file},
};

#[tokio::main]
async fn main() -> Result<()> {
	let config = Config::init()?;

	let db = HandleDatabase::new(&config).run().await?;

	if let Command::Import(path) = &config.command {
		return HandleImport::new(db, path).run().await;
	}

	let vendors = HandleVendors::new(&config).run();

	if config.command == Command::PortScan {
		return HandlePortScan::new(db, &config, vendors).run().await;
	}

	HandleScans::new(db.clone(), &config, vendors).run();
	HandleBaselines::new(db.clone()).run();

	let state = HttpServerState {
		db,
		test_file: config.test_file_bytes.map(test_file::generate),
		ui_dir: config.ui_dir.clone(),
	};
	HandleServerApi::new(&config, state).await?.run().await?;

	Err(Error::MainLoopClosed)
}
