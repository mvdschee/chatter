use axum::{
	Json, Router,
	extract::{DefaultBodyLimit, State, rejection::JsonRejection},
	http::StatusCode,
	routing::get,
};

use crate::{
	core::{
		handlers::floorplan::{load_plan, save_plan},
		models::floorplan::{FloorPlan, FloorPlanPayload},
	},
	error, info,
	services::web::HttpServerState,
};

/// A plan at its limits (100 rooms of 100 corners) is well under this.
const MAX_BODY: usize = 512 * 1024;

pub fn routes() -> Router<HttpServerState> {
	Router::new().route(
		"/plan",
		get(plan_get_handler).put(plan_put_handler).layer(DefaultBodyLimit::max(MAX_BODY)),
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
