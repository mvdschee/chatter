use std::{
	path::{Path, PathBuf},
	time::{SystemTime, UNIX_EPOCH},
};

use sqlx::Row;

use crate::{
	Error, Result,
	core::models::floorplan::{FloorPlan, MAX_UNDERLAY_BYTES, underlay_type},
	repository::{
		db::{DatabaseRepository, DbValue},
		schemas::{FLOORPLANS_INSERT, FLOORPLANS_LATEST},
	},
	services::filesystem,
};

/// The underlay image, one file in the data directory: a new one replaces it.
pub fn underlay_path(data_dir: &Path) -> PathBuf {
	data_dir.join("plan-underlay")
}

/// The newest saved plan; `None` before the first save.
pub async fn load_plan(db: &DatabaseRepository) -> Result<Option<FloorPlan>> {
	let Some(row) = db.get_record(FLOORPLANS_LATEST, vec![]).await? else {
		return Ok(None);
	};
	let json: String = row.try_get("plan")?;
	serde_json::from_str(&json)
		.map(Some)
		.map_err(|e| Error::Database(format!("stored floor plan: {e}")))
}

/// Saves the plan as a new row; the older ones stay. Returns its id.
pub async fn save_plan(db: &DatabaseRepository, plan: &FloorPlan) -> Result<i64> {
	let json = serde_json::to_string(plan).map_err(|e| Error::Plan(format!("to JSON: {e}")))?;
	let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
	db.insert_record(FLOORPLANS_INSERT, vec![DbValue::Text(json), DbValue::Int(now)]).await
}

/// Stores a new underlay image, if it's a PNG, JPEG or WebP and not too big.
pub fn save_underlay(data_dir: &Path, bytes: &[u8]) -> Result<()> {
	if bytes.len() > MAX_UNDERLAY_BYTES {
		return Err(Error::Plan(format!("an underlay is at most {} MB", MAX_UNDERLAY_BYTES >> 20)));
	}
	underlay_type(bytes)?;
	filesystem::write(&underlay_path(data_dir), bytes)
}

/// The underlay image and its content type; `None` before the first one.
pub fn load_underlay(data_dir: &Path) -> Result<Option<(&'static str, Vec<u8>)>> {
	let Some(bytes) = filesystem::read(&underlay_path(data_dir))? else {
		return Ok(None);
	};
	Ok(Some((underlay_type(&bytes)?, bytes)))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::models::floorplan::FloorPlanPayload;

	#[test]
	fn an_underlay_is_stored_and_replaced() {
		let dir =
			std::env::temp_dir().join(format!("chatter-test-underlay-{}", std::process::id()));
		std::fs::create_dir_all(&dir).unwrap();

		assert_eq!(load_underlay(&dir).unwrap(), None, "nothing before the first one");
		let png = b"\x89PNG\r\n\x1a\n made-up".to_vec();
		save_underlay(&dir, &png).unwrap();
		assert_eq!(load_underlay(&dir).unwrap(), Some(("image/png", png)));
		let jpeg = b"\xff\xd8\xff made-up".to_vec();
		save_underlay(&dir, &jpeg).unwrap();
		assert_eq!(load_underlay(&dir).unwrap(), Some(("image/jpeg", jpeg.clone())));
		assert!(save_underlay(&dir, b"<svg></svg>").is_err());
		assert_eq!(
			load_underlay(&dir).unwrap(),
			Some(("image/jpeg", jpeg)),
			"a refused one changes nothing"
		);

		std::fs::remove_dir_all(&dir).unwrap();
	}

	#[tokio::test]
	async fn the_newest_save_is_the_plan() {
		let path =
			std::env::temp_dir().join(format!("chatter-test-floorplan-{}.db", std::process::id()));
		let db = DatabaseRepository::new(path.to_str().unwrap()).await.unwrap();
		db.init().await.unwrap();

		assert_eq!(load_plan(&db).await.unwrap(), None, "nothing before the first save");

		let plan = |json: &str| {
			FloorPlan::try_from(serde_json::from_str::<FloorPlanPayload>(json).unwrap()).unwrap()
		};
		let first =
			plan(r#"{"rooms":[{"id":"r1","name":"Den","points":[[0,0],[2.4,0],[2.4,1.8]]}]}"#);
		let second = plan(
			r#"{"rooms":[{"id":"r1","name":"Den","points":[[0,0],[2.4,0],[2.4,1.8],[0,1.8]]}],
			"openings":[{"from":[2.4,0],"to":[2.4,1.8],"at":0.5,"width":0.8}]}"#,
		);
		let a = save_plan(&db, &first).await.unwrap();
		let b = save_plan(&db, &second).await.unwrap();
		assert!(b > a);
		assert_eq!(load_plan(&db).await.unwrap(), Some(second));

		for suffix in ["", "-wal", "-shm"] {
			let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
		}
	}
}
