use std::{
	collections::HashMap,
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use sqlx::Row;

use crate::{
	Result,
	core::{
		handlers::overview::latest_report,
		models::baseline::{Minute, UsualTraffic, WINDOW_S, baselines, hour_of, usual_traffic},
	},
	error, info,
	repository::{
		db::{DatabaseRepository, DbValue},
		schemas::{
			BASELINES_ALL, BASELINES_DELETE_ALL, BASELINES_INSERT, DEVICE_MINUTES_BYTES_BETWEEN,
			REPORTS_BETWEEN, SCANS_PORT_SCANS_BETWEEN,
		},
	},
};

/// Works out every device's baselines over the 14 days up to the newest
/// report and replaces the stored ones. Returns how many devices have one.
pub async fn compute_baselines(db: &DatabaseRepository) -> Result<usize> {
	let to = latest_report(db).await? + 1;
	let between = || vec![DbValue::Int(to - WINDOW_S), DbValue::Int(to)];

	let minutes = db
		.get_records(REPORTS_BETWEEN, between())
		.await?
		.iter()
		.map(|row| {
			let at = row.try_get("received_at")?;
			Ok(Minute {
				report: row.try_get("id")?,
				at,
				hour: hour_of(at),
			})
		})
		.collect::<Result<Vec<_>>>()?;
	let heard = db
		.get_records(DEVICE_MINUTES_BYTES_BETWEEN, between())
		.await?
		.iter()
		.map(|row| Ok((row.try_get("report_id")?, row.try_get("mac")?, row.try_get("bytes")?)))
		.collect::<Result<Vec<_>>>()?;
	let scans = db
		.get_records(SCANS_PORT_SCANS_BETWEEN, between())
		.await?
		.iter()
		.map(|row| Ok((row.try_get("started_at")?, row.try_get("finished_at")?)))
		.collect::<Result<Vec<_>>>()?;

	let devices = baselines(&minutes, &heard, &scans);

	let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
	let mut statements = vec![(BASELINES_DELETE_ALL, vec![])];
	for d in &devices {
		for (hour, b) in d.hours.iter().chain([&d.all]).enumerate() {
			statements.push((
				BASELINES_INSERT,
				vec![
					DbValue::Text(d.mac.clone()),
					DbValue::Int(hour as i64),
					DbValue::Real(b.median),
					DbValue::Real(b.mad),
					DbValue::Int(b.n as i64),
					DbValue::Int(now),
				],
			));
		}
	}
	db.execute_all(statements).await?;
	Ok(devices.len())
}

/// Every device's stored baselines, for the page.
pub async fn load_usual_traffic(db: &DatabaseRepository) -> Result<HashMap<String, UsualTraffic>> {
	let rows = db
		.get_records(BASELINES_ALL, vec![])
		.await?
		.iter()
		.map(|row| {
			Ok((
				row.try_get("mac")?,
				row.try_get("hour")?,
				row.try_get("median")?,
				row.try_get("mad")?,
				row.try_get("n")?,
			))
		})
		.collect::<Result<Vec<_>>>()?;
	Ok(usual_traffic(&rows))
}

/// Works out the baselines now, then every hour. A failure is logged and the
/// loop goes on.
pub async fn baseline_loop(db: DatabaseRepository) {
	loop {
		match compute_baselines(&db).await {
			Ok(devices) => info!("baselines: {devices} devices"),
			Err(e) => error!("baselines: {e}"),
		}
		tokio::time::sleep(Duration::from_secs(3600)).await;
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::{
		handlers::report::store_report,
		models::report::{Report, ReportPayload},
	};

	#[tokio::test]
	async fn the_baselines_are_stored_and_replaced() {
		let path =
			std::env::temp_dir().join(format!("chatter-test-baselines-{}.db", std::process::id()));
		let db = DatabaseRepository::new(path.to_str().unwrap()).await.unwrap();
		db.init().await.unwrap();

		// two hours of minutes; the device is heard in every other one
		for seq in 0..120 {
			let devices = if seq % 2 == 0 {
				r#"[{"mac":"c6:61:9e:67:da:88","net":0,"tx_frames":1,"tx_bytes":600,"rx_frames":1,"rx_bytes":400,"retries":0,"rssi_max":-50}]"#
			} else {
				"[]"
			};
			let mut payload: ReportPayload = serde_json::from_str(&format!(
				r#"{{"sensor":"c6-1","seq":{seq},"uptime_s":{},"channel":6,"dropped":0,"overflow":0,"nets_known":[0],"devices":{devices},"broadcast":[]}}"#,
				60 * (seq + 1)
			))
			.unwrap();
			payload.received_at = 1_700_000_000 + 60 * seq;
			store_report(&db, &Report::try_from(payload).unwrap()).await.unwrap();
		}

		for _ in 0..2 {
			assert_eq!(compute_baselines(&db).await.unwrap(), 1);
		}
		let rows = db
			.get_records(
				"SELECT mac, sum(n) AS n, max(median) AS median FROM baselines GROUP BY mac",
				vec![],
			)
			.await
			.unwrap();
		assert_eq!(rows.len(), 1, "replaced, not added to");
		assert_eq!(rows[0].try_get::<i64, _>("n").unwrap(), 2 * 120, "the hours and all of them");
		// half the minutes at 1000 bytes and half at 0: the median between them
		let median: f64 = rows[0].try_get("median").unwrap();
		assert!((median - 1_001f64.ln() / 2.0).abs() < 1e-9);

		for suffix in ["", "-wal", "-shm"] {
			let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
		}
	}
}
