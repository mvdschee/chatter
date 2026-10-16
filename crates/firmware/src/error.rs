use esp_radio::wifi::WifiError;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, defmt::Format)]
pub enum Error {
	Wifi(WifiError),
	/// Posting a report failed; the text says at which step.
	Report(&'static str),
}

impl From<WifiError> for Error {
	fn from(e: WifiError) -> Self {
		Error::Wifi(e)
	}
}
