use serde_json::Value;

use crate::{
	Error, Result,
	core::{
		handlers::report::{Ingested, store_report},
		models::report::{Report, ReportPayload},
	},
	repository::db::DatabaseRepository,
	warn,
};

#[derive(Debug, Default, PartialEq)]
pub struct ImportSummary {
	pub stored: usize,
	pub duplicates: usize,
	pub bad: usize,
}

/// Loads reports from JSON Lines, as the first collector wrote them: one
/// report per line, plus `received_unix`, when it arrived. Reports stored
/// already are skipped, so a newer copy of the same file can be imported
/// again. A bad line is logged and counted, and the import goes on.
pub async fn import_reports(db: &DatabaseRepository, lines: &str) -> Result<ImportSummary> {
	let mut summary = ImportSummary::default();
	for (n, line) in lines.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
		match parse_line(line) {
			Ok(report) => match store_report(db, &report).await? {
				Ingested::Stored {
					..
				} => summary.stored += 1,
				Ingested::Duplicate => summary.duplicates += 1,
			},
			Err(e) => {
				warn!("line {}: {e}", n + 1);
				summary.bad += 1;
			},
		}
	}
	Ok(summary)
}

fn parse_line(line: &str) -> Result<Report> {
	let value: Value =
		serde_json::from_str(line).map_err(|e| Error::Report(format!("bad JSON: {e}")))?;
	let received_at =
		value["received_unix"].as_i64().ok_or_else(|| Error::Report("no received_unix".into()))?;
	let mut payload: ReportPayload =
		serde_json::from_value(value).map_err(|e| Error::Report(format!("not a report: {e}")))?;
	payload.received_at = received_at;
	Report::try_from(payload)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn takes_the_receive_time_from_the_line() {
		let line = r#"{"sensor":"c6-1","seq":3,"uptime_s":180,"channel":6,"dropped":0,"overflow":0,"nets_known":[0],"devices":[],"broadcast":[],"received_unix":1700000123}"#;
		let r = parse_line(line).unwrap();
		assert_eq!((r.seq, r.received_at), (3, 1_700_000_123));
	}

	#[test]
	fn rejects_a_line_without_a_receive_time() {
		let line = r#"{"sensor":"c6-1","seq":3,"uptime_s":180,"channel":6,"dropped":0,"overflow":0,"nets_known":[0],"devices":[],"broadcast":[]}"#;
		assert!(parse_line(line).is_err());
		assert!(parse_line("{").is_err());
	}
}
