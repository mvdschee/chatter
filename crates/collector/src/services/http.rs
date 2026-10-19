use std::{path::Path, time::Duration};

use reqwest::Client;

use crate::{Error, Result};

/// A generic User-Agent: nothing about whoever runs the collector.
const USER_AGENT: &str = "chatter";

pub fn create_client() -> Result<Client> {
	Client::builder()
		.user_agent(USER_AGENT)
		.timeout(Duration::from_secs(120))
		.build()
		.map_err(|e| Error::Http(format!("client: {e}")))
}

/// Downloads `url` to `path`, through a temporary file next to it, so a
/// failed download leaves the old copy as it was.
pub async fn download(client: &Client, url: &str, path: &Path) -> Result<()> {
	let body = client
		.get(url)
		.send()
		.await
		.and_then(|r| r.error_for_status())
		.map_err(|e| Error::Http(format!("{url}: {e}")))?
		.bytes()
		.await
		.map_err(|e| Error::Http(format!("{url}: {e}")))?;
	let tmp = path.with_extension("tmp");
	tokio::fs::write(&tmp, &body)
		.await
		.map_err(|e| Error::FileSystem(format!("write {}: {e}", tmp.display())))?;
	tokio::fs::rename(&tmp, path)
		.await
		.map_err(|e| Error::FileSystem(format!("rename to {}: {e}", path.display())))
}
