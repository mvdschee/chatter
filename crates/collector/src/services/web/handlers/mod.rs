use axum::{Router, http::StatusCode, routing::get};

use super::HttpServerState;

pub mod overview;
pub mod report;
pub mod test_file;
pub mod ui;

pub fn root_routes() -> Router<HttpServerState> {
	Router::new().route("/healthcheck", get(healthcheck_get_handler))
}

// SECURITY: public on purpose, says nothing but "up".
async fn healthcheck_get_handler() -> StatusCode {
	StatusCode::OK
}
