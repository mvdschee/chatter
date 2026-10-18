use axum::{Json, Router, extract::State, http::StatusCode, routing::get};

use crate::{
	core::{handlers::overview::overview, models::overview::Overview},
	error, info,
	services::web::HttpServerState,
};

pub fn routes() -> Router<HttpServerState> {
	Router::new().route("/overview", get(overview_get_handler))
}

// SECURITY: unauthenticated, like the whole collector for now (LAN only, see
// DECISIONS.md). It lists every device's MAC and traffic: it must not be
// reachable from outside the LAN.
async fn overview_get_handler(
	State(state): State<HttpServerState>,
) -> Result<Json<Overview>, StatusCode> {
	info!("GET /v1/overview");
	overview(&state.db).await.map(Json).map_err(|e| {
		error!("overview: {e}");
		StatusCode::INTERNAL_SERVER_ERROR
	})
}
