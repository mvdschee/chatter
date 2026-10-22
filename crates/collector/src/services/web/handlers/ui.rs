use axum::{
	Router,
	extract::{Path, State},
	http::{StatusCode, header},
	response::IntoResponse,
	routing::get,
};

use crate::{error, services::web::HttpServerState};

macro_rules! js {
	($name:literal) => {
		($name, "text/javascript; charset=utf-8", include_str!(concat!("../../../../ui/", $name)))
	};
}

/// The monitoring page's files, built into the binary: name, content type,
/// content. The scripts are ES modules, main.js imports the rest.
const FILES: [(&str, &str, &str); 18] = [
	("index.html", "text/html; charset=utf-8", include_str!("../../../../ui/index.html")),
	("style.css", "text/css; charset=utf-8", include_str!("../../../../ui/style.css")),
	("icons.svg", "image/svg+xml", include_str!("../../../../ui/icons.svg")),
	js!("main.js"),
	js!("data.js"),
	js!("plan.js"),
	js!("geometry.js"),
	js!("edit.js"),
	js!("view.js"),
	js!("draw.js"),
	js!("playback.js"),
	js!("timeline.js"),
	js!("panel.js"),
	js!("sensors.js"),
	js!("identify.js"),
	js!("colour.js"),
	js!("format.js"),
	js!("random.js"),
];

pub fn routes() -> Router<HttpServerState> {
	Router::new()
		.route("/", get(|state| ui_get_handler(state, "index.html".to_string())))
		.route("/{file}", get(|state, Path(file): Path<String>| ui_get_handler(state, file)))
}

// SECURITY: public static files; the data comes from /v1/overview.
async fn ui_get_handler(
	State(state): State<HttpServerState>,
	name: String,
) -> Result<impl IntoResponse, StatusCode> {
	// only the files above, so a name can't reach anything else on disk
	let Some((name, content_type, built_in)) = FILES.iter().find(|(file, _, _)| *file == name)
	else {
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn every_file_in_ui_is_served() {
		let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");
		let mut on_disk: Vec<String> = std::fs::read_dir(dir)
			.unwrap()
			.map(|e| e.unwrap().file_name().into_string().unwrap())
			.collect();
		let mut served: Vec<String> = FILES.iter().map(|(name, _, _)| name.to_string()).collect();
		on_disk.sort();
		served.sort();
		assert_eq!(
			served, on_disk,
			"a file in ui/ that FILES doesn't list (or the other way round)"
		);
	}
}
