use std::net::SocketAddr;

use axum::{
	Json, Router,
	extract::{ConnectInfo, DefaultBodyLimit, Path, State, rejection::JsonRejection},
	http::StatusCode,
	response::{IntoResponse, Response},
	routing::{get, post},
};

use crate::{
	core::{
		handlers::identify::{
			identify_left, identify_view, record_readings, start_identify, store_session, unix_now,
		},
		models::{
			identify::{IdentifyView, Readings, ReadingsPayload, Start, StartPayload},
			report::SensorName,
		},
	},
	error, info,
	services::web::HttpServerState,
};

/// A post is at most 2 KiB (core's `IDENTIFY_BUF`).
const MAX_BODY: usize = 8 * 1024;

/// Unversioned: the firmware asks `/mode/{sensor}` at boot and posts to
/// `/identify`.
pub fn routes() -> Router<HttpServerState> {
	Router::new().route("/mode/{sensor}", get(mode_get_handler)).route(
		"/identify",
		post(identify_create_post_handler).layer(DefaultBodyLimit::max(MAX_BODY)),
	)
}

pub fn v1_routes() -> Router<HttpServerState> {
	Router::new().route("/identify", get(identify_get_handler)).route(
		"/sensors/{sensor}/identify",
		post(identify_start_post_handler).layer(DefaultBodyLimit::max(4096)),
	)
}

/// The sensor's mode, as the firmware reads it (core's `parse_answer`):
/// `204` is "count", `200` with `identify <secs>\n` is "identify that long".
pub fn mode_answer(left: Option<u64>) -> Response {
	match left {
		Some(secs) => (StatusCode::OK, format!("identify {secs}\n")).into_response(),
		None => StatusCode::NO_CONTENT.into_response(),
	}
}

/// The mode for `sensor`, from the sessions.
pub fn answer_for(state: &HttpServerState, sensor: &SensorName, now: i64) -> Response {
	let mut identify = state.identify.lock().unwrap_or_else(|e| e.into_inner());
	mode_answer(identify_left(&mut identify, sensor, now))
}

// SECURITY: unauthenticated, like `/report`: says only whether a sensor
// should count or identify.
async fn mode_get_handler(
	State(state): State<HttpServerState>,
	Path(sensor): Path<String>,
) -> Response {
	info!("GET /mode/{sensor}");
	match SensorName::new(&sensor) {
		Ok(sensor) => answer_for(&state, &sensor, unix_now()),
		Err(e) => {
			error!("mode: {e}");
			StatusCode::BAD_REQUEST.into_response()
		},
	}
}

// SECURITY: unauthenticated, like `/report`: anyone on the LAN can post
// readings for a sensor in a session.
async fn identify_create_post_handler(
	State(state): State<HttpServerState>,
	payload: Result<Json<ReadingsPayload>, JsonRejection>,
) -> Response {
	let mut payload = match payload {
		Ok(Json(payload)) => payload,
		Err(e) => {
			error!("identify: {e}");
			return e.status().into_response();
		},
	};
	let now = unix_now();
	payload.received_at = now;
	let readings = match Readings::try_from(payload) {
		Ok(readings) => readings,
		Err(e) => {
			error!("identify: {e}");
			return StatusCode::BAD_REQUEST.into_response();
		},
	};
	match readings.heard.first() {
		Some(h) => info!(
			"POST /identify {}: {} heard, strongest {} at {}",
			&*readings.sensor,
			readings.heard.len(),
			&*h.mac,
			h.rssi_max
		),
		None => info!("POST /identify {}: nothing heard", &*readings.sensor),
	}
	let mut identify = state.identify.lock().unwrap_or_else(|e| e.into_inner());
	mode_answer(record_readings(&mut identify, readings, now))
}

// SECURITY: unauthenticated, like the overview (LAN only, see DECISIONS.md).
/// The sessions, what their sensors hear now, and who's online. The page's
/// own device is found by the IP it asks from.
async fn identify_get_handler(
	State(state): State<HttpServerState>,
	ConnectInfo(from): ConnectInfo<SocketAddr>,
) -> Json<IdentifyView> {
	let identify = state.identify.lock().unwrap_or_else(|e| e.into_inner());
	Json(identify_view(&identify, Some(from.ip().to_canonical()), unix_now()))
}

// SECURITY: unauthenticated, like the plan (LAN only, see DECISIONS.md):
// anyone on the LAN can start or stop identify mode, which stops a sensor
// counting until it's rebooted.
/// Starts identify mode for `secs` seconds, or stops it with 0: 204, or 400
/// with why it was refused.
async fn identify_start_post_handler(
	State(state): State<HttpServerState>,
	Path(sensor): Path<String>,
	payload: Result<Json<StartPayload>, JsonRejection>,
) -> Result<StatusCode, (StatusCode, String)> {
	info!("POST /v1/sensors/{sensor}/identify");
	let Json(mut payload) = payload.map_err(|e| (e.status(), e.body_text()))?;
	payload.sensor = sensor;
	let start = Start::try_from(payload).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
	let session = {
		let mut identify = state.identify.lock().unwrap_or_else(|e| e.into_inner());
		start_identify(&mut identify, &start, unix_now())
	};
	if let Some(session) = session {
		store_session(&state.db, &session).await.map_err(|e| {
			error!("identify session: {e}");
			(StatusCode::INTERNAL_SERVER_ERROR, String::new())
		})?;
	}
	Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
	use super::*;

	async fn render(left: Option<u64>) -> (StatusCode, String) {
		let r = mode_answer(left);
		let status = r.status();
		let body = axum::body::to_bytes(r.into_body(), 1024).await.unwrap();
		(status, String::from_utf8(body.to_vec()).unwrap())
	}

	// the firmware's `parse_answer` reads exactly these
	#[tokio::test]
	async fn answers_the_firmware_reads() {
		assert_eq!(render(None).await, (StatusCode::NO_CONTENT, String::new()));
		assert_eq!(render(Some(600)).await, (StatusCode::OK, "identify 600\n".into()));
	}
}
