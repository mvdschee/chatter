use sqlx::Row;

use crate::{
	Result,
	core::models::report::Report,
	info,
	repository::{
		db::{DatabaseRepository, DbValue},
		schemas::{
			BROADCAST_MINUTES_INSERT, DEVICE_MINUTES_INSERT, DEVICE_STREAMS_UPSERT, REPORTS_FIND,
			REPORTS_FIND_IN_BOOT, REPORTS_INSERT, REPORTS_LAST_SEQ,
		},
	},
	warn,
};

/// For firmware without a boot id: how far apart a report and its resend can
/// arrive. The sensor's outbox holds 16 reports, a minute each, so a resend
/// is never more than about 16 minutes late. A reboot repeats `seq` and
/// uptime, so a match from further away is another boot's report; one from
/// a reboot within the hour is still taken for a resend, which the boot id
/// rules out.
const RESEND_WINDOW_S: i64 = 3600;

#[derive(Debug, PartialEq)]
pub enum Ingested {
	/// With what the sequence number says about the reports in between.
	Stored {
		id: i64,
		gap: String,
	},
	/// Stored before: the sensor resent it because our answer got lost.
	Duplicate,
}

/// Stores a report and logs one line for it. A resend is answered as if
/// stored, so the sensor drops it.
pub async fn ingest_report(db: &DatabaseRepository, report: Report) -> Result<Ingested> {
	let ingested = store_report(db, &report).await?;
	match &ingested {
		Ingested::Stored {
			gap,
			..
		} => {
			let rx: i64 = report.devices.iter().map(|d| d.rx_bytes).sum();
			let tx: i64 = report.devices.iter().map(|d| d.tx_bytes).sum();
			info!(
				"{} seq {} uptime {}s ch {}: {} devices, rx {rx} B, tx {tx} B, dropped {}, overflow {}{gap}",
				*report.sensor,
				report.seq,
				report.uptime_s,
				report.channel,
				report.devices.len(),
				report.dropped,
				report.overflow,
			);
		},
		Ingested::Duplicate => {
			warn!("{} seq {}: resent, already stored", *report.sensor, report.seq)
		},
	}
	Ok(ingested)
}

/// Stores a report with its device and broadcast counters, unless it's
/// stored already: the same `seq` from the same boot.
pub async fn store_report(db: &DatabaseRepository, report: &Report) -> Result<Ingested> {
	let sensor = DbValue::Text(report.sensor.to_string());
	let found = match report.boot {
		Some(boot) => {
			db.get_record(
				REPORTS_FIND_IN_BOOT,
				vec![
					DbValue::Text(report.sensor.to_string()),
					DbValue::Int(boot),
					DbValue::Int(report.seq),
				],
			)
			.await?
		},
		None => {
			db.get_record(
				REPORTS_FIND,
				vec![
					DbValue::Text(report.sensor.to_string()),
					DbValue::Int(report.seq),
					DbValue::Int(report.uptime_s),
					DbValue::Int(report.received_at - RESEND_WINDOW_S),
					DbValue::Int(report.received_at + RESEND_WINDOW_S),
				],
			)
			.await?
		},
	};
	if found.is_some() {
		return Ok(Ingested::Duplicate);
	}

	let last = db.get_record(REPORTS_LAST_SEQ, vec![sensor]).await?;
	let last_seq = last.map(|row| row.try_get::<i64, _>("seq")).transpose()?;
	let gap = gap_note(last_seq, report.seq);

	let mut children = Vec::with_capacity(report.devices.len() + report.broadcast.len());
	for d in &report.devices {
		children.push((
			DEVICE_MINUTES_INSERT,
			vec![
				DbValue::Text(d.mac.to_string()),
				DbValue::Int(d.net.into()),
				DbValue::Int(report.received_at),
				DbValue::Int(d.tx_frames),
				DbValue::Int(d.tx_bytes),
				DbValue::Int(d.rx_frames),
				DbValue::Int(d.rx_bytes),
				DbValue::Int(d.retries),
				DbValue::IntOpt(d.rssi_max.map(i64::from)),
			],
		));
		if let Some(count) = &d.streams {
			children.push((
				DEVICE_STREAMS_UPSERT,
				vec![
					DbValue::Text(d.mac.to_string()),
					DbValue::Int(count.streams),
					DbValue::Text(count.from.name().to_string()),
					DbValue::Int(report.received_at),
				],
			));
		}
	}
	for b in &report.broadcast {
		children.push((
			BROADCAST_MINUTES_INSERT,
			vec![DbValue::Int(b.net.into()), DbValue::Int(b.rx_frames), DbValue::Int(b.rx_bytes)],
		));
	}
	let nets_known =
		format!("[{}]", report.nets_known.iter().map(u8::to_string).collect::<Vec<_>>().join(","));
	let id = db
		.insert_record_with_children(
			REPORTS_INSERT,
			vec![
				DbValue::Text(report.sensor.to_string()),
				DbValue::Int(report.seq),
				DbValue::Int(report.uptime_s),
				DbValue::Int(report.received_at),
				DbValue::Int(report.channel.into()),
				DbValue::Int(report.dropped),
				DbValue::Int(report.overflow),
				DbValue::Text(nets_known),
				DbValue::IntOpt(report.boot),
			],
			children,
		)
		.await?;
	Ok(Ingested::Stored {
		id,
		gap,
	})
}

/// What the sequence number says about the reports in between.
pub fn gap_note(last: Option<i64>, seq: i64) -> String {
	match last {
		Some(prev) if seq == prev + 1 => String::new(),
		Some(prev) if seq > prev => format!("  <- gap: expected {}", prev + 1),
		Some(_) => "  <- seq went back: sensor restarted".into(),
		None => String::new(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{core::models::report::ReportPayload, repository::schemas::REPORTS_BETWEEN};

	#[test]
	fn gap_notes() {
		assert_eq!(gap_note(None, 0), "");
		assert_eq!(gap_note(Some(4), 5), "");
		assert_eq!(gap_note(Some(4), 7), "  <- gap: expected 5");
		assert_eq!(gap_note(Some(4), 0), "  <- seq went back: sensor restarted");
	}

	fn report_at(received_at: i64) -> Report {
		report_in(None, received_at)
	}

	fn report_in(boot: Option<u32>, received_at: i64) -> Report {
		let mut payload: ReportPayload = serde_json::from_str(
			r#"{"sensor":"c6-1","seq":0,"uptime_s":60,"channel":6,"dropped":0,"overflow":0,"nets_known":[0],
			"devices":[{"mac":"aa:bb:cc:00:00:01","net":0,"tx_frames":1,"tx_bytes":100,"rx_frames":1,"rx_bytes":100,"retries":0,"rssi_max":-50}],
			"broadcast":[]}"#,
		)
		.unwrap();
		payload.boot = boot;
		payload.received_at = received_at;
		Report::try_from(payload).unwrap()
	}

	#[tokio::test]
	async fn a_reboot_within_the_hour_is_stored_with_a_boot_id() {
		let path =
			std::env::temp_dir().join(format!("chatter-test-boot-{}.db", std::process::id()));
		let _ = std::fs::remove_file(&path);
		let db = DatabaseRepository::new(path.to_str().unwrap()).await.unwrap();
		db.init().await.unwrap();

		let t = 1_700_000_000;
		let stored = |i: Ingested| matches!(i, Ingested::Stored { .. });
		assert!(stored(store_report(&db, &report_in(Some(7), t)).await.unwrap()));
		assert_eq!(
			store_report(&db, &report_in(Some(7), t + 300)).await.unwrap(),
			Ingested::Duplicate
		);
		// rebooted minutes later: the same seq and uptime, another boot
		assert!(stored(store_report(&db, &report_in(Some(8), t + 300)).await.unwrap()));

		let reports = db
			.get_records(REPORTS_BETWEEN, vec![DbValue::Int(0), DbValue::Int(i64::MAX)])
			.await
			.unwrap();
		assert_eq!(reports.len(), 2);
		let _ = std::fs::remove_file(&path);
	}

	#[tokio::test]
	async fn a_resend_is_dropped_but_a_reboot_is_stored() {
		let path =
			std::env::temp_dir().join(format!("chatter-test-resend-{}.db", std::process::id()));
		let _ = std::fs::remove_file(&path);
		let db = DatabaseRepository::new(path.to_str().unwrap()).await.unwrap();
		db.init().await.unwrap();

		let t = 1_700_000_000;
		assert!(matches!(store_report(&db, &report_at(t)).await.unwrap(), Ingested::Stored { .. }));
		// the answer got lost; the outbox resends it a few minutes later
		assert_eq!(store_report(&db, &report_at(t + 300)).await.unwrap(), Ingested::Duplicate);
		// a later boot's first report: the same seq and uptime
		assert!(matches!(
			store_report(&db, &report_at(t + 2 * RESEND_WINDOW_S)).await.unwrap(),
			Ingested::Stored { .. }
		));

		let reports = db
			.get_records(REPORTS_BETWEEN, vec![DbValue::Int(0), DbValue::Int(i64::MAX)])
			.await
			.unwrap();
		assert_eq!(reports.len(), 2);
		let _ = std::fs::remove_file(&path);
	}

	/// A report with one device, and `streams` after its `rssi_max`.
	fn report_streams(seq: u32, received_at: i64, streams: &str) -> Report {
		let mut payload: ReportPayload = serde_json::from_str(&format!(
			r#"{{"sensor":"c6-1","boot":1,"seq":{seq},"uptime_s":60,"channel":6,"dropped":0,"overflow":0,"nets_known":[0],
			"devices":[{{"mac":"aa:bb:cc:00:00:01","net":0,"tx_frames":1,"tx_bytes":100,"rx_frames":1,"rx_bytes":100,"retries":0,"rssi_max":-50{streams}}}],
			"broadcast":[]}}"#,
		))
		.unwrap();
		payload.received_at = received_at;
		Report::try_from(payload).unwrap()
	}

	/// The MAC's row: streams, source, `first_at`, `assoc_at`.
	async fn stored_streams(db: &DatabaseRepository) -> Option<(i64, String, i64, Option<i64>)> {
		const ONE: &str = "SELECT streams, streams_from, first_at, assoc_at FROM device_streams";
		db.get_record(ONE, vec![]).await.unwrap().map(|row| {
			(row.get("streams"), row.get("streams_from"), row.get("first_at"), row.get("assoc_at"))
		})
	}

	#[tokio::test]
	async fn which_stream_count_wins() {
		let path =
			std::env::temp_dir().join(format!("chatter-test-streams-{}.db", std::process::id()));
		let _ = std::fs::remove_file(&path);
		let db = DatabaseRepository::new(path.to_str().unwrap()).await.unwrap();
		db.init().await.unwrap();

		let t = 1_700_000_000;
		let probe = |n: u32| format!(r#","streams":{n},"streams_from":"probe""#);
		let assoc = |n: u32| format!(r#","streams":{n},"streams_from":"assoc""#);

		// (what the report says, the count, the source, assoc_at after it)
		let steps: Vec<(String, i64, &str, Option<i64>)> = vec![
			(probe(2), 2, "probe", None),
			// a reboot: the sensor knows nothing yet
			(String::new(), 2, "probe", None),
			(probe(1), 2, "probe", None),
			(probe(3), 3, "probe", None),
			// lower, but an association request's
			(assoc(1), 1, "assoc", Some(t + 4)),
			(probe(4), 1, "assoc", Some(t + 4)),
			(String::new(), 1, "assoc", Some(t + 4)),
			(assoc(2), 2, "assoc", Some(t + 4)),
		];
		for (i, (says, streams, from, assoc_at)) in steps.into_iter().enumerate() {
			let report = report_streams(i as u32, t + i as i64, &says);
			assert!(matches!(store_report(&db, &report).await.unwrap(), Ingested::Stored { .. }));
			assert_eq!(
				stored_streams(&db).await,
				Some((streams, from.to_string(), t, assoc_at)),
				"after {says:?}"
			);
		}

		// a resend isn't stored again, so it can't bring an old count back
		let resend = report_streams(4, t + 100, &assoc(1));
		assert_eq!(store_report(&db, &resend).await.unwrap(), Ingested::Duplicate);
		assert_eq!(stored_streams(&db).await.unwrap().0, 2);
		let _ = std::fs::remove_file(&path);
	}

	#[tokio::test]
	async fn a_report_without_counts_stores_none() {
		let path =
			std::env::temp_dir().join(format!("chatter-test-no-streams-{}.db", std::process::id()));
		let _ = std::fs::remove_file(&path);
		let db = DatabaseRepository::new(path.to_str().unwrap()).await.unwrap();
		db.init().await.unwrap();

		store_report(&db, &report_at(1_700_000_000)).await.unwrap();
		assert_eq!(stored_streams(&db).await, None);
		let _ = std::fs::remove_file(&path);
	}

	/// Test 2 in docs/streams.md: how long until each member has a count,
	/// with probe requests and without.
	#[tokio::test]
	async fn the_stored_counts_answer_how_long_until_seen() {
		let path =
			std::env::temp_dir().join(format!("chatter-test-seen-{}.db", std::process::id()));
		let _ = std::fs::remove_file(&path);
		let db = DatabaseRepository::new(path.to_str().unwrap()).await.unwrap();
		db.init().await.unwrap();

		let t = 1_700_000_000;
		store_report(&db, &report_streams(0, t, "")).await.unwrap();
		let probe = r#","streams":2,"streams_from":"probe""#;
		store_report(&db, &report_streams(1, t + 60, probe)).await.unwrap();
		let assoc = r#","streams":2,"streams_from":"assoc""#;
		store_report(&db, &report_streams(2, t + 300, assoc)).await.unwrap();

		const HOW_LONG: &str = "SELECT s.first_at - m.heard AS with_probes, \
			s.assoc_at - m.heard AS without_probes FROM device_streams s JOIN \
			(SELECT mac, min(received_at) AS heard FROM device_minutes GROUP BY mac) m \
			ON m.mac = s.mac";
		let row = db.get_record(HOW_LONG, vec![]).await.unwrap().unwrap();
		assert_eq!(row.get::<i64, _>("with_probes"), 60);
		assert_eq!(row.get::<i64, _>("without_probes"), 300);
		let _ = std::fs::remove_file(&path);
	}
}
