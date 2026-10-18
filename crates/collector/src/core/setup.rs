use crate::{
	Result,
	core::{handlers::import::import_reports, models::config::Config},
	info,
	repository::db::DatabaseRepository,
	services::{
		filesystem,
		web::{HttpServer, HttpServerConfig, HttpServerState},
	},
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

/// The HTTP server: the sensors' reports, and the test client's download.
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

/// Loads a file of reports into the database.
pub struct HandleImport {
	db: DatabaseRepository,
	path: String,
}

impl HandleImport {
	pub fn new(db: DatabaseRepository, path: &str) -> Self {
		Self {
			db,
			path: path.to_string(),
		}
	}

	pub async fn run(self) -> Result<()> {
		info!("Importing {}...", self.path);
		let lines = filesystem::read_to_string(&self.path)?;
		let summary = import_reports(&self.db, &lines).await?;
		info!(
			"imported {}: {} stored, {} already there, {} bad",
			self.path, summary.stored, summary.duplicates, summary.bad
		);
		Ok(())
	}
}
