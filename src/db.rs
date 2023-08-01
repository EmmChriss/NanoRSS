use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{app::AppUser, App, Error, Result};

#[derive(Serialize, Deserialize)]
pub struct NewUser {
	pub username: String,
	pub password: String,
}

impl NewUser {
	pub fn insert(self, app: &App) -> Result<User> {
		if app.users.contains_key(self.username.as_bytes())? {
			return Err(Error::UsernameTaken);
		}

		let pass_hash = bcrypt::hash(self.password.as_bytes(), 10)?;
		let user = User {
			username: self.username,
			pass_hash,
		};

		app.users
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
	pub async fn insert(self, app: &AppUser) -> Result<()> {
		Feed {
			id: app.db.generate_id()?,
			url: self.url,
			name: self.name.unwrap_or_default(),
			scraper: self.scraper,

			last_fetch_time: DateTime::<Utc>::MIN_UTC,
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
	pub fn apply(self, app: &AppUser) -> Result<()> {
		let mut feed = Feed::get_id(app, self.id)?.ok_or(Error::NotFound("feed".into()))?;

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

	pub last_fetch_time: DateTime<Utc>,
	pub last_error: Option<String>,
}

impl Feed {
	pub fn insert(&self, app: &AppUser) -> Result<()> {
		app.feeds
			.insert(bincode::serialize(&self.id)?, bincode::serialize(&self)?)?;
		Ok(())
	}

	pub fn get_id(app: &AppUser, id: u64) -> Result<Option<Feed>> {
		let maybe_feed = app.feeds.get(bincode::serialize(&id)?)?;

		let feed = if let Some(feed) = maybe_feed {
			bincode::deserialize(&feed)?
		}
		else {
			None
		};

		Ok(feed)
	}

	pub fn get_all(app: &AppUser) -> Result<Vec<Feed>> {
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
	pub feed_id: u64,
	pub published: DateTime<Utc>,
	pub url: Option<String>,
	pub title: String,
	pub summary: String,
	pub content: String,
}

impl Article {
	pub fn get_id(app: &AppUser, id: &str) -> Result<Option<Article>> {
		let maybe = app.articles.get(bincode::serialize(&id)?)?;

		let article = if let Some(feed) = maybe {
			bincode::deserialize(&feed)?
		}
		else {
			None
		};

		Ok(article)
	}

	pub fn insert(&self, app: &AppUser) -> Result<()> {
		app.articles
			.insert(self.id.as_bytes(), bincode::serialize(self)?)
			.map(|_| ())
			.map_err(Error::from)
	}

	pub fn iter(app: &AppUser) -> impl Iterator<Item = Result<Article>> {
		app.articles.iter().map(|item| {
			item.map_err(Error::from)
				.and_then(|(_, v)| bincode::deserialize::<Article>(&v).map_err(Error::from))
		})
	}

	pub fn get_all(app: &AppUser) -> Result<Vec<Article>> {
		Article::iter(app).collect()
	}
}

impl indicium::simple::Indexable for Article {
	fn strings(&self) -> Vec<String> {
		return vec![
			self.title.clone(),
			self.summary.clone(),
			self.content.clone(),
		];
	}
}

#[non_exhaustive]
pub enum ImportOpts {
	Opml(opml::OPML),
}

pub async fn import(app: &AppUser, opts: ImportOpts) -> Result<()> {
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

#[derive(Deserialize, Debug)]
#[serde(tag = "kind")]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExportOpts {
	Opml,
}

pub fn export(app: &AppUser, opts: ExportOpts) -> Result<String> {
	match opts {
		ExportOpts::Opml => {
			let mut opml = opml::OPML::default();
			for feed in Feed::get_all(app)? {
				opml.add_feed(&feed.name, &feed.url.to_string());
			}

			opml.to_string().map_err(Error::from)
		}
	}
}
