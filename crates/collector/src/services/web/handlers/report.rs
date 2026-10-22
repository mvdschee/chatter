use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
	Json, Router,
	extract::{DefaultBodyLimit, State, rejection::JsonRejection},
	http::StatusCode,
	response::{IntoResponse, Response},
	routing::post,
};

use crate::{
	core::{
		handlers::report::ingest_report,
		models::report::{Report, ReportPayload},
	},
	error, info,
	services::web::{HttpServerState, handlers::identify::answer_for},
};

/// A report is at most 24 KiB (core's `REPORT_BUF`); refuse anything much bigger.
const MAX_BODY: usize = 64 * 1024;

/// Unversioned: the firmware posts to `/report`.
pub fn routes() -> Router<HttpServerState> {
	Router::new()
		.route("/report", post(report_create_post_handler).layer(DefaultBodyLimit::max(MAX_BODY)))
}

// SECURITY: unauthenticated, like the whole collector for now: anyone on the
// LAN can post a report. TODO: a per-sensor token once sensors are set up
// without reflashing.
/// Stores the report and answers with the sensor's mode (see
/// `identify::mode_answer`).
async fn report_create_post_handler(
	State(state): State<HttpServerState>,
	payload: Result<Json<ReportPayload>, JsonRejection>,
) -> Response {
	info!("POST /report");
	let mut payload = match payload {
		Ok(Json(payload)) => payload,
		Err(e) => {
			error!("report: {e}");
			return e.status().into_response();
		},
	};
	payload.received_at =
		SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
	let report = match Report::try_from(payload) {
		Ok(report) => report,
		Err(e) => {
			error!("report: {e}");
			return StatusCode::BAD_REQUEST.into_response();
		},
	};
	let (sensor, now) = (report.sensor.clone(), report.received_at);
	match ingest_report(&state.db, report).await {
		Ok(_) => answer_for(&state, &sensor, now),
		Err(e) => {
			error!("report: {e}");
			StatusCode::INTERNAL_SERVER_ERROR.into_response()
		},
	}
}
