use std::time::Duration;

use crate::err::Result;

pub enum FetchConfig {
	Feed,
	Scrape { scraper_config: String },
}

pub struct Fetcher {
	client: reqwest::Client,
}

impl Fetcher {
	pub fn new() -> Result<Self> {
		let client = reqwest::ClientBuilder::new()
			.timeout(Duration::from_secs(20))
			.connect_timeout(Duration::from_secs(10))
			.build()?;

		Ok(Fetcher { client })
	}

	pub async fn fetch(&self, url: &str, cfg: FetchConfig) -> Result<()> {
		let response = self
			.client
			.get(url)
			.send()
			.await?
			.error_for_status()?
			.bytes()
			.await?;

		// NOTE: this might appear redundant, but Rust couldn't figure out the types otherwise
		let response_byteslice: &[u8] = &response;
		let parsed = feed_rs::parser::parse_with_uri(response_byteslice, None)?;

		Ok(())
	}
}
