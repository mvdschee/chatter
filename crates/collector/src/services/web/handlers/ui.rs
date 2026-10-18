use axum::{
	Router,
	extract::State,
	http::{StatusCode, header},
	response::IntoResponse,
	routing::get,
};

use crate::{error, services::web::HttpServerState};

/// The monitoring page's files, built into the binary: name, content type,
/// content.
const FILES: [(&str, &str, &str); 3] = [
	("index.html", "text/html; charset=utf-8", include_str!("../../../../ui/index.html")),
	("style.css", "text/css; charset=utf-8", include_str!("../../../../ui/style.css")),
	("app.js", "text/javascript; charset=utf-8", include_str!("../../../../ui/app.js")),
];

pub fn routes() -> Router<HttpServerState> {
	Router::new()
		.route("/", get(|state| ui_get_handler(state, "index.html")))
		.route("/style.css", get(|state| ui_get_handler(state, "style.css")))
		.route("/app.js", get(|state| ui_get_handler(state, "app.js")))
}

// SECURITY: public static files; the data comes from /v1/overview.
async fn ui_get_handler(
	State(state): State<HttpServerState>,
	name: &'static str,
) -> Result<impl IntoResponse, StatusCode> {
	let Some((_, content_type, built_in)) = FILES.iter().find(|(file, _, _)| *file == name) else {
		return Err(StatusCode::NOT_FOUND);
	};
	// CHATTER_UI_DIR: read the file from disk on every request, so a refresh
	// shows the latest edit
	let body = match &state.ui_dir {
		Some(dir) => std::fs::read_to_string(format!("{dir}/{name}")).map_err(|e| {
			error!("ui: {dir}/{name}: {e}");
			StatusCode::INTERNAL_SERVER_ERROR
		})?,
		None => built_in.to_string(),
	};
	Ok(([(header::CONTENT_TYPE, *content_type), (header::CACHE_CONTROL, "no-cache")], body))
}
