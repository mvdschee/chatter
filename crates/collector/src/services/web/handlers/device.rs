use axum::{
	Json, Router,
	extract::{DefaultBodyLimit, Path, State, rejection::JsonRejection},
	http::StatusCode,
	routing::put,
};

use crate::{
	core::{
		handlers::device::rename_device,
		models::device::{DeviceName, DeviceNamePayload},
	},
	error, info,
	services::web::HttpServerState,
};

pub fn routes() -> Router<HttpServerState> {
	Router::new()
		.route("/devices/{mac}/name", put(name_put_handler).layer(DefaultBodyLimit::max(4096)))
}

// SECURITY: unauthenticated, like the plan (LAN only, see DECISIONS.md):
// anyone on the LAN can rename a device. The page writes names as text, not
// HTML.
/// Gives a device a name, or takes it away with an empty one: 204, or 400
/// with why it was refused.
async fn name_put_handler(
	State(state): State<HttpServerState>,
	Path(mac): Path<String>,
	payload: Result<Json<DeviceNamePayload>, JsonRejection>,
) -> Result<StatusCode, (StatusCode, String)> {
	info!("PUT /v1/devices/{mac}/name");
	let Json(mut payload) = payload.map_err(|e| (e.status(), e.body_text()))?;
	payload.mac = mac;
	let named =
		DeviceName::try_from(payload).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
	rename_device(&state.db, &named).await.map(|_| StatusCode::NO_CONTENT).map_err(|e| {
		error!("device name: {e}");
		(StatusCode::INTERNAL_SERVER_ERROR, String::new())
	})
}
