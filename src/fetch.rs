use chrono::Utc;
use futures::stream::TryStreamExt;

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
	let utc_now = Utc::now();
	for entry in parsed.entries {
		// NOTE: we might be getting an error here because the scema does not parse anymore
		let prev_article = match Article::get_id(app, &entry.id) {
			Ok(a) => a,
			Err(e) => {
				log::warn!("could not get article from db: {}", e);
				None
			}
		};
		Article {
			id: entry.id,
			feed_id: feed.id,
			url: entry
				.content
				.as_ref()
				.and_then(|content| content.src.as_ref().map(|link| link.href.clone()))
				.or_else(|| entry.links.first().map(|link| link.href.clone())),
			title: entry.title.map(|text| text.content).unwrap_or_default(),
			summary: entry.summary.map(|text| text.content).unwrap_or_default(),
			published: entry
				.published
				.or_else(|| prev_article.map(|article| article.published))
				.unwrap_or(utc_now),
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

			feed.last_fetch_time = Utc::now();
			feed.last_error = result.err().map(|e| format!("{}", e));

			feed.insert(app)?;

			Ok::<_, Error>(())
		})
		.await?;

	// create search index
	app.create_search_index()?;

	Ok(())
}
