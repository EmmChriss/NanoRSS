use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

use indicium::simple::SearchIndex;

use crate::db::Article;
use crate::err::Result;

pub struct Config {
	pub db_path: PathBuf,
}

pub struct App {
	db: sled::Db,
	pub users: sled::Tree,
	client: reqwest::Client,
}

impl App {
	const TREE_USERS: &str = "users";
	const TREE_FEEDS: &str = "feeds";
	const TREE_ARTICLES: &str = "articles";
	const TREE_INDEX: &str = "index";

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

		Ok(Self { db, users, client })
	}

	pub fn open_user(&self, username: &str) -> Result<AppUser> {
		let db = self.db.clone();
		let feeds = self
			.db
			.open_tree(format!("{}/{}", username, Self::TREE_FEEDS))?;
		let articles = self
			.db
			.open_tree(format!("{}/{}", username, Self::TREE_ARTICLES))?;
		let search_index = self
			.db
			.open_tree(format!("{}/{}", username, Self::TREE_INDEX))?;

		Ok(AppUser {
			db,
			feeds,
			articles,
			client: self.client.clone(),
		})
	}
}

pub struct AppUser {
	pub db: sled::Db,
	pub feeds: sled::Tree,
	pub articles: sled::Tree,
	pub client: reqwest::Client,
}

impl AppUser {
	pub fn search(&self, term: &str) -> Result<Vec<String>> {
		// reconstruct search index from sled
		let b_tree: BTreeMap<String, BTreeSet<String>> = self
			.articles
			.get(b"__search_index")?
			.map(|bytes| bincode::deserialize(&bytes))
			.transpose()?
			.unwrap_or_default();

		// hackly replace search index b_tree_map
		let mut search_index = indicium::simple::SearchIndexBuilder::default().build();
		*search_index = b_tree;

		// search results
		Ok(search_index
			.search(term)
			.into_iter()
			.map(ToOwned::to_owned)
			.collect())
	}

	pub fn create_search_index(&self) -> Result<()> {
		// create index
		let mut search_index = indicium::simple::SearchIndexBuilder::default().build();
		for article in Article::get_all(self)? {
			search_index.insert(&article.id, &article);
		}

		// manually serialize search index into db
		self.articles
			.insert(b"__search_index", bincode::serialize(&*search_index)?)?;

		Ok(())
	}
}
