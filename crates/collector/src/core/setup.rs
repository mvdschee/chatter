use crate::{
	Result,
	core::models::config::Config,
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
