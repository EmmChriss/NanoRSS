use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use url::Url;

use crate::{App, Error, Result};

#[derive(Serialize, Deserialize)]
pub struct NewUser {
	pub username: String,
	pub password: String,
}

impl NewUser {
	pub fn insert(self, db: &App) -> Result<User> {
		if db.users.contains_key(self.username.as_bytes())? {
			return Err(Error::UsernameTaken);
		}

		let pass_hash = bcrypt::hash(self.password.as_bytes(), 10)?;
		let user = User {
			username: self.username,
			pass_hash,
		};

		db.users
			.insert(user.username.as_bytes(), bincode::serialize(&user)?)?;

		Ok(user)
	}
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct User {
	pub username: String,
	pub pass_hash: String,
}

impl User {
	fn get_user(db: &App, username: &str) -> Result<Option<User>> {
		db.users
			.get(username.as_bytes())?
			.map(|bytes| bincode::deserialize(&bytes))
			.transpose()
			.map_err(Into::into)
	}

	pub fn try_login(db: &App, username: &str, password: &str) -> Result<User> {
		let user = Self::get_user(db, username)?.ok_or(Error::UsernameNotFound)?;

		// validate password
		if !bcrypt::verify(password, &user.pass_hash)? {
			return Err(Error::PasswordIncorrect);
		}

		Ok(user)
	}
}

#[derive(Serialize, Deserialize)]
pub struct ScraperConfig {}

#[derive(Serialize, Deserialize)]
pub struct NewFeed {
	pub url: url::Url,
	pub name: Option<String>,
	pub scraper: Option<ScraperConfig>,
}

impl NewFeed {
	pub async fn insert(self, app: &App) -> Result<()> {
		Feed {
			id: app.db.generate_id()?,
			url: self.url,
			name: self.name.unwrap_or_default(),
			scraper: self.scraper,

			last_fetch_time: OffsetDateTime::UNIX_EPOCH,
			last_error: None,
		}
		.insert(app)?;

		Ok(())
	}
}

#[derive(Deserialize)]
pub struct PatchFeed {
	pub id: u64,
	pub url: Option<url::Url>,
	pub name: Option<String>,
	pub scraper: Option<Option<ScraperConfig>>,
}

impl PatchFeed {
	pub fn apply(self, app: &App) -> Result<()> {
		let mut feed = Feed::get_feed(app, self.id)?.ok_or(Error::NotFound("feed".into()))?;

		if let Some(url) = self.url {
			feed.url = url;
		}
		if let Some(name) = self.name {
			feed.name = name;
		}
		if let Some(scraper) = self.scraper {
			feed.scraper = scraper;
		}

		Ok(())
	}
}

#[derive(Serialize, Deserialize)]
pub struct Feed {
	pub id: u64,
	pub url: url::Url,
	pub name: String,
	pub scraper: Option<ScraperConfig>,

	pub last_fetch_time: OffsetDateTime,
	pub last_error: Option<String>,
}

impl Feed {
	pub fn insert(&self, app: &App) -> Result<()> {
		app.feeds
			.insert(bincode::serialize(&self.id)?, bincode::serialize(&self)?)?;
		Ok(())
	}

	pub fn get_feed(app: &App, id: u64) -> Result<Option<Feed>> {
		let maybe_feed = app.feeds.get(bincode::serialize(&id)?)?;

		let feed = if let Some(feed) = maybe_feed {
			bincode::deserialize(&feed)?
		} else {
			None
		};

		Ok(feed)
	}

	pub fn get_all(app: &App) -> Result<Vec<Feed>> {
		app.feeds
			.iter()
			.map(|item| {
				item.map_err(Error::from)
					.and_then(|(_, v)| bincode::deserialize(&v).map_err(Error::from))
			})
			.collect()
	}
}

#[derive(Serialize, Deserialize)]
pub struct Article {
	pub id: String,
	pub title: String,
	pub summary: String,
	pub content: String,
}

impl Article {
	pub fn insert(&self, app: &App) -> Result<()> {
		app.articles
			.insert(self.id.as_bytes(), bincode::serialize(self)?)
			.map(|_| ())
			.map_err(Error::from)
	}

	pub fn get_all(app: &App) -> Result<Vec<Article>> {
		app.articles
			.iter()
			.map(|item| {
				item.map_err(Error::from)
					.and_then(|(_, v)| bincode::deserialize(&v).map_err(Error::from))
			})
			.collect()
	}
}

pub enum ImportOpts {
	Opml(opml::OPML),
}

pub async fn import(app: &App, opts: ImportOpts) -> Result<()> {
	match opts {
		ImportOpts::Opml(opml) => {
			fn walk_outlines(outline: opml::Outline, collector: &mut Vec<opml::Outline>) {
				for outline in &outline.outlines {
					walk_outlines(outline.clone(), collector);
				}

				collector.push(outline);
			}

			let mut vec = Vec::new();
			for outline in opml.body.outlines {
				walk_outlines(outline, &mut vec);
			}

			let is_feed = |o: &opml::Outline| o.xml_url.is_some();

			for outline in vec {
				if is_feed(&outline) {
					NewFeed {
						url: Url::parse(&outline.xml_url.unwrap_or_default())?,
						name: Some(outline.text),
						scraper: None,
					}
					.insert(app)
					.await?;
				}
			}

			Ok(())
		}
	}
}

pub enum ExportOpts {
	Opml,
}

pub fn export(app: &App, opts: ExportOpts) -> Result<String> {
	todo!()
}
