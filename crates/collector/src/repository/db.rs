use std::str::FromStr;

use sqlx::{
	Row, Sqlite, SqlitePool,
	query::Query,
	sqlite::{
		SqliteArguments, SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow,
	},
};

use crate::{Error, Result, info};

#[derive(Clone)]
pub struct DatabaseRepository {
	pool: SqlitePool,
}

pub enum DbValue {
	Text(String),
	TextOpt(Option<String>),
	Int(i64),
	IntOpt(Option<i64>),
}

type SqliteQuery<'q> = Query<'q, Sqlite, SqliteArguments>;

fn bind(mut query: SqliteQuery<'_>, params: Vec<DbValue>) -> SqliteQuery<'_> {
	for param in params {
		query = match param {
			DbValue::Text(value) => query.bind(value),
			DbValue::TextOpt(value) => query.bind(value),
			DbValue::Int(value) => query.bind(value),
			DbValue::IntOpt(value) => query.bind(value),
		};
	}
	query
}

impl DatabaseRepository {
	/// Opens the database file at `path`, creating it if it's missing.
	pub async fn new(path: &str) -> Result<Self> {
		info!("Opening database {path}...");
		let options = SqliteConnectOptions::from_str(path)
			.map_err(|e| Error::Database(format!("database path {path}: {e}")))?
			.create_if_missing(true)
			.journal_mode(SqliteJournalMode::Wal)
			.foreign_keys(true);
		let pool = SqlitePoolOptions::new()
			.connect_with(options)
			.await
			.map_err(|e| Error::Database(format!("open {path}: {e}")))?;
		Ok(Self {
			pool,
		})
	}

	pub async fn init(&self) -> Result<()> {
		info!("Migrating tables...");
		sqlx::migrate!()
			.run(&self.pool)
			.await
			.map_err(|e| Error::Database(format!("migration: {e}")))
	}

	/// Insert with dynamic SQL and arguments; the SQL ends in `RETURNING id`.
	pub async fn insert_record(&self, sql: &'static str, params: Vec<DbValue>) -> Result<i64> {
		let row = bind(sqlx::query(sql), params)
			.fetch_one(&self.pool)
			.await
			.map_err(|e| Error::Database(format!("insert record: {e}")))?;
		Ok(row.try_get("id")?)
	}

	/// Inserts a record and its children in one transaction: all of them or
	/// none. The parent's SQL ends in `RETURNING id`; each child's SQL takes
	/// that id as `?1`, before its own params.
	pub async fn insert_record_with_children(
		&self,
		sql: &'static str,
		params: Vec<DbValue>,
		children: Vec<(&'static str, Vec<DbValue>)>,
	) -> Result<i64> {
		let mut tx = self.pool.begin().await.map_err(|e| Error::Database(format!("begin: {e}")))?;
		let row = bind(sqlx::query(sql), params)
			.fetch_one(&mut *tx)
			.await
			.map_err(|e| Error::Database(format!("insert record: {e}")))?;
		let id: i64 = row.try_get("id")?;
		for (child_sql, child_params) in children {
			bind(sqlx::query(child_sql).bind(id), child_params)
				.execute(&mut *tx)
				.await
				.map_err(|e| Error::Database(format!("insert child record: {e}")))?;
		}
		tx.commit().await.map_err(|e| Error::Database(format!("commit: {e}")))?;
		Ok(id)
	}

	/// Fetch one record dynamically.
	pub async fn get_record(
		&self,
		sql: &'static str,
		params: Vec<DbValue>,
	) -> Result<Option<SqliteRow>> {
		bind(sqlx::query(sql), params)
			.fetch_optional(&self.pool)
			.await
			.map_err(|e| Error::Database(format!("get record: {e}")))
	}

	/// Fetch multiple records dynamically.
	pub async fn get_records(
		&self,
		sql: &'static str,
		params: Vec<DbValue>,
	) -> Result<Vec<SqliteRow>> {
		bind(sqlx::query(sql), params)
			.fetch_all(&self.pool)
			.await
			.map_err(|e| Error::Database(format!("get records: {e}")))
	}
}
