use axum::{
	Router,
	body::Bytes,
	extract::State,
	http::{StatusCode, header},
	response::IntoResponse,
	routing::get,
};

use crate::{info, services::web::HttpServerState};

pub fn routes() -> Router<HttpServerState> {
	Router::new().route("/test.bin", get(test_file_get_handler))
}

/// `bytes` of xorshift output: not compressible, and the same every run.
pub fn generate(bytes: usize) -> Bytes {
	let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
	let mut out = Vec::with_capacity(bytes + 8);
	while out.len() < bytes {
		state ^= state << 13;
		state ^= state >> 7;
		state ^= state << 17;
		out.extend_from_slice(&state.to_le_bytes());
	}
	out.truncate(bytes);
	Bytes::from(out)
}

// SECURITY: public, made-up bytes; only served when CHATTER_TEST_FILE_MIB is set.
async fn test_file_get_handler(State(state): State<HttpServerState>) -> impl IntoResponse {
	info!("GET /test.bin");
	match state.test_file {
		Some(bytes) => Ok(([(header::CONTENT_TYPE, "application/octet-stream")], bytes)),
		None => Err(StatusCode::NOT_FOUND),
	}
}
