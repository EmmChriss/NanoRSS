use futures::stream::TryStreamExt;
use time::{OffsetDateTime, UtcOffset};

use crate::{
	app::AppUser,
	db::{Article, Feed},
	err::Result,
	Error,
};

// TODO: implement scraper
pub async fn fetch_feed(app: &AppUser, feed: &Feed) -> Result<()> {
	let response = app
		.client
		.get(feed.url.clone())
		.send()
		.await?
		.error_for_status()?
		.bytes()
		.await?;

	// NOTE: this might appear redundant, but Rust couldn't figure out the types otherwise
	let response_byteslice: &[u8] = &response;
	let parsed = feed_rs::parser::parse_with_uri(response_byteslice, Some(feed.url.as_str()))?;

	// insert new stuff
	for entry in parsed.entries {
		Article {
			id: entry.id,
			title: entry.title.map(|text| text.content).unwrap_or_default(),
			summary: entry.summary.map(|text| text.content).unwrap_or_default(),
			content: entry
				.content
				.map(|content| content.body.unwrap_or_default())
				.unwrap_or_default(),
		}
		.insert(app)?;
	}

	Ok(())
}

pub async fn fetch_all_feeds(app: &AppUser) -> Result<()> {
	// do these concurrently
	futures::stream::iter(Feed::get_all(&app)?.into_iter().map(Ok))
		.try_for_each_concurrent(32, |mut feed| async move {
			let result = fetch_feed(app, &feed).await;

			// attempt to take local time with timezone
			// fall back to just taking local datetime
			let time = OffsetDateTime::now_local()
				.map(|t| t.to_offset(UtcOffset::UTC))
				.unwrap_or_else(|_| OffsetDateTime::now_utc());

			feed.last_fetch_time = time;
			feed.last_error = result.err().map(|e| format!("{}", e));

			feed.insert(app)?;

			Ok::<_, Error>(())
		})
		.await?;

	// TODO: create search index

	Ok(())
}
