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

#[derive(Debug, PartialEq)]
pub enum Ingested {
	Stored(i64),
	/// Stored before: the sensor resent it because our answer got lost.
	Duplicate,
}

/// Stores a report with its device and broadcast counters, and logs one line
/// for it. A resend is answered as if stored, so the sensor drops it.
pub async fn ingest_report(db: &DatabaseRepository, report: Report) -> Result<Ingested> {
	let sensor = DbValue::Text(report.sensor.to_string());
	let found = db
		.get_record(
			REPORTS_FIND,
			vec![
				DbValue::Text(report.sensor.to_string()),
				DbValue::Int(report.seq),
				DbValue::Int(report.uptime_s),
			],
		)
		.await?;
	if found.is_some() {
		warn!("{} seq {}: resent, already stored", *report.sensor, report.seq);
		return Ok(Ingested::Duplicate);
	}

	let last = db.get_record(REPORTS_LAST_SEQ, vec![sensor]).await?;
	let last_seq = last.map(|row| row.try_get::<i64, _>("seq")).transpose()?;
	let note = gap_note(last_seq, report.seq);

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

	let rx: i64 = report.devices.iter().map(|d| d.rx_bytes).sum();
	let tx: i64 = report.devices.iter().map(|d| d.tx_bytes).sum();
	info!(
		"{} seq {} uptime {}s ch {}: {} devices, rx {rx} B, tx {tx} B, dropped {}, overflow {}{note}",
		*report.sensor,
		report.seq,
		report.uptime_s,
		report.channel,
		report.devices.len(),
		report.dropped,
		report.overflow,
	);
	Ok(Ingested::Stored(id))
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

	#[test]
	fn gap_notes() {
		assert_eq!(gap_note(None, 0), "");
		assert_eq!(gap_note(Some(4), 5), "");
		assert_eq!(gap_note(Some(4), 7), "  <- gap: expected 5");
		assert_eq!(gap_note(Some(4), 0), "  <- seq went back: sensor restarted");
	}
}
