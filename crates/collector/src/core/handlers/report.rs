use sqlx::Row;

use crate::{
	Result,
	core::models::report::Report,
	info,
	repository::{
		db::{DatabaseRepository, DbValue},
		schemas::{
			BROADCAST_MINUTES_INSERT, DEVICE_MINUTES_INSERT, REPORTS_FIND, REPORTS_INSERT,
			REPORTS_LAST_SEQ,
		},
	},
	warn,
};

/// How far apart a report and its resend can arrive. The sensor's outbox
/// holds 16 reports, a minute each, so a resend is never more than about 16
/// minutes late. A reboot repeats `seq` and uptime, so a match from further
/// away is another boot's report, not a resend.
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
/// stored already.
pub async fn store_report(db: &DatabaseRepository, report: &Report) -> Result<Ingested> {
	let sensor = DbValue::Text(report.sensor.to_string());
	let found = db
		.get_record(
			REPORTS_FIND,
			vec![
				DbValue::Text(report.sensor.to_string()),
				DbValue::Int(report.seq),
				DbValue::Int(report.uptime_s),
				DbValue::Int(report.received_at - RESEND_WINDOW_S),
				DbValue::Int(report.received_at + RESEND_WINDOW_S),
			],
		)
		.await?;
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
		let mut payload: ReportPayload = serde_json::from_str(
			r#"{"sensor":"c6-1","seq":0,"uptime_s":60,"channel":6,"dropped":0,"overflow":0,"nets_known":[0],
			"devices":[{"mac":"aa:bb:cc:00:00:01","net":0,"tx_frames":1,"tx_bytes":100,"rx_frames":1,"rx_bytes":100,"retries":0,"rssi_max":-50}],
			"broadcast":[]}"#,
		)
		.unwrap();
		payload.received_at = received_at;
		Report::try_from(payload).unwrap()
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
		// importing an older log again finds the first copy, not the reboot's
		assert_eq!(store_report(&db, &report_at(t)).await.unwrap(), Ingested::Duplicate);

		let reports = db
			.get_records(REPORTS_BETWEEN, vec![DbValue::Int(0), DbValue::Int(i64::MAX)])
			.await
			.unwrap();
		assert_eq!(reports.len(), 2);
		let _ = std::fs::remove_file(&path);
	}
}
