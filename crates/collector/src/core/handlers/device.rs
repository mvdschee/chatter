use std::{
	collections::HashMap,
	time::{SystemTime, UNIX_EPOCH},
};

use sqlx::Row;

use crate::{
	Result,
	core::models::device::DeviceName,
	repository::{
		db::{DatabaseRepository, DbValue},
		schemas::{DEVICE_NAMES_ALL, DEVICE_NAMES_DELETE, DEVICE_NAMES_UPSERT},
	},
};

/// Gives the device its name, or takes it away.
pub async fn rename_device(db: &DatabaseRepository, named: &DeviceName) -> Result<()> {
	let mac = DbValue::Text(named.mac.clone());
	let statement = match &named.name {
		Some(name) => {
			let now =
				SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
			(DEVICE_NAMES_UPSERT, vec![mac, DbValue::Text(name.clone()), DbValue::Int(now)])
		},
		None => (DEVICE_NAMES_DELETE, vec![mac]),
	};
	db.execute_all(vec![statement]).await
}

/// Every name given, by MAC.
pub async fn device_names(db: &DatabaseRepository) -> Result<HashMap<String, String>> {
	db.get_records(DEVICE_NAMES_ALL, vec![])
		.await?
		.iter()
		.map(|row| Ok((row.try_get("mac")?, row.try_get("name")?)))
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn a_name_is_given_changed_and_taken_away() {
		let path =
			std::env::temp_dir().join(format!("chatter-test-names-{}.db", std::process::id()));
		let db = DatabaseRepository::new(path.to_str().unwrap()).await.unwrap();
		db.init().await.unwrap();
		let mac = "ea:5f:12:c4:9b:07".to_string();
		let name = |n: Option<&str>| DeviceName {
			mac: mac.clone(),
			name: n.map(Into::into),
		};

		assert!(device_names(&db).await.unwrap().is_empty());
		rename_device(&db, &name(Some("Desk lamp"))).await.unwrap();
		rename_device(&db, &name(Some("Hall lamp"))).await.unwrap();
		assert_eq!(
			device_names(&db).await.unwrap().get(&mac).map(String::as_str),
			Some("Hall lamp")
		);
		rename_device(&db, &name(None)).await.unwrap();
		assert!(device_names(&db).await.unwrap().is_empty());

		for suffix in ["", "-wal", "-shm"] {
			let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
		}
	}
}
