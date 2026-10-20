use std::path::PathBuf;

use axum::{Router, body::Bytes};
use tokio::{net::TcpListener, signal};

use crate::{Error, Result, info, repository::db::DatabaseRepository};

pub mod handlers;

#[derive(Debug, Clone)]
pub struct HttpServerConfig<'a> {
	pub bind: &'a str,
}

#[derive(Clone)]
pub struct HttpServerState {
	pub db: DatabaseRepository,
	/// The test client's download; `None` serves no `GET /test.bin`.
	pub test_file: Option<Bytes>,
	/// Development: serve the page from this directory instead of the binary.
	pub ui_dir: Option<String>,
	/// Where the collector keeps its files (the plan's underlay image).
	pub data_dir: PathBuf,
}

pub struct HttpServer {
	router: Router,
	listener: TcpListener,
}

impl HttpServer {
	pub async fn new(config: HttpServerConfig<'_>, state: HttpServerState) -> Result<Self> {
		// No authentication yet: the collector listens on the LAN only (see
		// DECISIONS.md, "Collector on the LAN without authentication").
		let app = Router::new()
			.merge(handlers::root_routes())
			.merge(handlers::report::routes())
			.merge(handlers::test_file::routes())
			.merge(handlers::ui::routes())
			.nest(
				"/v1",
				handlers::overview::routes()
					.merge(handlers::plan::routes())
					.merge(handlers::timeline::routes()),
			)
			.with_state(state);

		let listener = TcpListener::bind(config.bind)
			.await
			.map_err(|e| Error::Config(format!("can't listen on {}: {e}", config.bind)))?;
		info!("Listener bound to http://{}/", config.bind);

		Ok(Self {
			router: app,
			listener,
		})
	}

	pub async fn run(self) -> Result<()> {
		info!("http service running");
		axum::serve(self.listener, self.router).with_graceful_shutdown(shutdown()).await?;
		Ok(())
	}
}

/// Ctrl-C, or SIGTERM from `docker stop`.
async fn shutdown() {
	let ctrl_c = async {
		let _ = signal::ctrl_c().await;
	};
	let term = async {
		match signal::unix::signal(signal::unix::SignalKind::terminate()) {
			Ok(mut s) => {
				s.recv().await;
			},
			Err(_) => std::future::pending().await,
		}
	};
	tokio::select! {
		() = ctrl_c => {},
		() = term => {},
	}
	info!("shutting down");
}
