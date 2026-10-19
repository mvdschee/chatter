use axum::{
	Json, Router,
	extract::{Query, State},
	http::StatusCode,
	routing::get,
};
use serde::Deserialize;

use crate::{
	core::{
		handlers::{overview::latest_report, timeline::timeline},
		models::timeline::{MAX_SPAN_S, Timeline, Window, range_seconds},
	},
	error, info,
	services::web::HttpServerState,
};

pub fn routes() -> Router<HttpServerState> {
	Router::new().route("/timeline", get(timeline_get_handler))
}

/// `?range=24h` (1h, 6h, 24h, 7d, 30d) ending at the newest report, or
/// `?from=…&to=…` in unix seconds (a zoomed-in stretch).
#[derive(Debug, Deserialize)]
struct TimelineParams {
	range: Option<String>,
	from: Option<i64>,
	to: Option<i64>,
}

// SECURITY: unauthenticated, like the whole collector for now (LAN only, see
// DECISIONS.md). It lists every device's MAC and traffic: it must not be
// reachable from outside the LAN.
async fn timeline_get_handler(
	State(state): State<HttpServerState>,
	Query(params): Query<TimelineParams>,
) -> Result<Json<Timeline>, StatusCode> {
	info!("GET /v1/timeline");
	let window = match (params.range.as_deref(), params.from, params.to) {
		(Some(range), None, None) => {
			let span = range_seconds(range).ok_or(StatusCode::BAD_REQUEST)?;
			let latest = latest_report(&state.db).await.map_err(|e| {
				error!("timeline: {e}");
				StatusCode::INTERNAL_SERVER_ERROR
			})?;
			Window::ending_at(latest, span)
		},
		(None, Some(from), Some(to)) if from < to && to - from <= MAX_SPAN_S => {
			Window::new(from, to)
		},
		_ => return Err(StatusCode::BAD_REQUEST),
	};
	timeline(&state.db, window).await.map(Json).map_err(|e| {
		error!("timeline: {e}");
		StatusCode::INTERNAL_SERVER_ERROR
	})
}
