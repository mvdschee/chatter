//! Collector: takes the sensors' reports on `POST /report` and stores them in
//! SQLite, one row per report and per device minute. With
//! `CHATTER_TEST_FILE_MIB` set it also serves `GET /test.bin` for the ESP32-C3
//! test client. Settings come from the environment, see `config.rs`.

pub use error::{Error, Result};

mod config;
mod core;
mod error;
mod repository;
mod services;
mod utils;

use crate::{
	core::{
		models::config::Config,
		setup::{HandleDatabase, HandleServerApi},
	},
	services::web::{HttpServerState, handlers::test_file},
};

#[tokio::main]
async fn main() -> Result<()> {
	let config = Config::init()?;

	let db = HandleDatabase::new(&config).run().await?;

	let state = HttpServerState {
		db,
		test_file: config.test_file_bytes.map(test_file::generate),
	};
	HandleServerApi::new(&config, state).await?.run().await?;

	Err(Error::MainLoopClosed)
}
