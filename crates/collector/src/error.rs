use thiserror::Error as DisplayError;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(DisplayError, Debug)]
pub enum Error {
	#[error("environment: `{0}` is not set")]
	Env(String),

	#[error("main loop closed")]
	MainLoopClosed,

	#[error("IO error: {0}")]
	Io(#[from] std::io::Error),

	#[error("config error: {0}")]
	Config(String),

	#[error("file system error: {0}")]
	FileSystem(String),

	#[error("database error: {0}")]
	Database(String),

	#[error("report error: {0}")]
	Report(String),

	#[error("scan error: {0}")]
	Scan(String),

	#[error("floor plan: {0}")]
	Plan(String),

	#[error("HTTP error: {0}")]
	Http(String),
}

impl From<sqlx::Error> for Error {
	fn from(err: sqlx::Error) -> Self {
		Error::Database(err.to_string())
	}
}
