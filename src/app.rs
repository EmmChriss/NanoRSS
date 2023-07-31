use std::path::PathBuf;
use std::time::Duration;

use crate::err::Result;
use crate::search::Searcher;

pub struct Config {
	pub db_path: PathBuf,
}

pub struct App {
	db: sled::Db,
	pub users: sled::Tree,
	client: reqwest::Client,
	searcher: Searcher,
}

pub struct AppUser {
	pub db: sled::Db,
	pub feeds: sled::Tree,
	pub articles: sled::Tree,
	pub client: reqwest::Client,
	pub searcher: Searcher,
}

impl App {
	const TREE_USERS: &str = "users";
	const TREE_FEEDS: &str = "feeds";
	const TREE_ARTICLES: &str = "articles";

	pub fn new(cfg: &Config) -> Result<Self> {
		let db = sled::Config::default()
			.path(&cfg.db_path)
			.flush_every_ms(Some(1000));

		let db = if cfg!(debug_assertions) {
			db.print_profile_on_drop(true)
		} else {
			db
		};

		let db = db.open()?;
		let users = db.open_tree(Self::TREE_USERS)?;

		let client = reqwest::ClientBuilder::new()
			.timeout(Duration::from_secs(20))
			.connect_timeout(Duration::from_secs(10))
			.build()?;

		let searcher = Searcher::new(&cfg.db_path)?;

		Ok(Self {
			db,
			users,
			client,
			searcher,
		})
	}

	pub fn open_user(&self, username: &str) -> Result<AppUser> {
		let db = self.db.clone();
		let feeds = self
			.db
			.open_tree(format!("{}/{}", username, Self::TREE_FEEDS))?;
		let articles = self
			.db
			.open_tree(format!("{}/{}", username, Self::TREE_ARTICLES))?;

		Ok(AppUser {
			db,
			feeds,
			articles,
			client: self.client.clone(),
			searcher: self.searcher.clone(),
		})
	}
}
