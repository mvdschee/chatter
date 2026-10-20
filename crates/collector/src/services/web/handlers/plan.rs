use axum::{
	Json, Router,
	body::Bytes,
	extract::{DefaultBodyLimit, State, rejection::JsonRejection},
	http::{StatusCode, header},
	response::IntoResponse,
	routing::get,
};

use crate::{
	Error,
	core::{
		handlers::floorplan::{load_plan, load_underlay, save_plan, save_underlay},
		models::floorplan::{FloorPlan, FloorPlanPayload, MAX_UNDERLAY_BYTES},
	},
	error, info,
	services::web::HttpServerState,
};

/// A plan at its limits (100 rooms of 100 corners) is well under this.
const MAX_BODY: usize = 512 * 1024;

pub fn routes() -> Router<HttpServerState> {
	Router::new()
		.route(
			"/plan",
			get(plan_get_handler).put(plan_put_handler).layer(DefaultBodyLimit::max(MAX_BODY)),
		)
		.route(
			"/plan/underlay",
			get(underlay_get_handler)
				.put(underlay_put_handler)
				.layer(DefaultBodyLimit::max(MAX_UNDERLAY_BYTES)),
		)
}

// SECURITY: unauthenticated, like the whole collector for now (LAN only, see
// DECISIONS.md).
/// The newest plan, or `null` before the first save.
async fn plan_get_handler(
	State(state): State<HttpServerState>,
) -> Result<Json<Option<FloorPlan>>, StatusCode> {
	info!("GET /v1/plan");
	load_plan(&state.db).await.map(Json).map_err(|e| {
		error!("plan: {e}");
		StatusCode::INTERNAL_SERVER_ERROR
	})
}

// SECURITY: unauthenticated, and the first route that changes what the page
// shows: anyone on the LAN can replace the plan. Every save is a new row, so
// an older plan can be restored.
/// Saves a new plan: 204, or 400 with why it was refused.
async fn plan_put_handler(
	State(state): State<HttpServerState>,
	payload: Result<Json<FloorPlanPayload>, JsonRejection>,
) -> Result<StatusCode, (StatusCode, String)> {
	info!("PUT /v1/plan");
	let Json(payload) = payload.map_err(|e| (e.status(), e.body_text()))?;
	let plan =
		FloorPlan::try_from(payload).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
	match save_plan(&state.db, &plan).await {
		Ok(_) => Ok(StatusCode::NO_CONTENT),
		Err(e) => {
			error!("plan: {e}");
			Err((StatusCode::INTERNAL_SERVER_ERROR, String::new()))
		},
	}
}

// SECURITY: unauthenticated, like the plan. Only PNG, JPEG or WebP, checked
// by their first bytes on the way in and out, and sent with nosniff, so
// nothing stored here runs as a page.
/// The underlay image, or 404 before the first one.
async fn underlay_get_handler(
	State(state): State<HttpServerState>,
) -> Result<impl IntoResponse, StatusCode> {
	info!("GET /v1/plan/underlay");
	match load_underlay(&state.data_dir) {
		Ok(Some((content_type, bytes))) => Ok((
			[
				(header::CONTENT_TYPE, content_type),
				(header::CACHE_CONTROL, "no-cache"),
				(header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
			],
			bytes,
		)),
		Ok(None) => Err(StatusCode::NOT_FOUND),
		Err(e) => {
			error!("underlay: {e}");
			Err(StatusCode::INTERNAL_SERVER_ERROR)
		},
	}
}

// SECURITY: unauthenticated: anyone on the LAN can replace the image. It
// only shows in the plan editor.
/// Stores a new underlay image, the body as it is: 204, or 400 with why it
/// was refused.
async fn underlay_put_handler(
	State(state): State<HttpServerState>,
	body: Bytes,
) -> Result<StatusCode, (StatusCode, String)> {
	info!("PUT /v1/plan/underlay ({} bytes)", body.len());
	match save_underlay(&state.data_dir, &body) {
		Ok(()) => Ok(StatusCode::NO_CONTENT),
		Err(e @ Error::Plan(_)) => Err((StatusCode::BAD_REQUEST, e.to_string())),
		Err(e) => {
			error!("underlay: {e}");
			Err((StatusCode::INTERNAL_SERVER_ERROR, String::new()))
		},
	}
}
