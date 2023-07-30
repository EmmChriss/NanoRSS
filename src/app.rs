use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tantivy::schema::*;
use tempfile::TempDir;

use crate::db::Article;
use crate::err::{Error, Result};
use crate::search::Searcher;

pub struct Config {
	pub db_path: PathBuf,
}

pub struct App {
	pub db: sled::Db,
	pub users: sled::Tree,
	pub feeds: sled::Tree,
	pub articles: sled::Tree,
	pub client: reqwest::Client,
	pub searcher: Searcher,
}

impl App {
	const TREE_USERS: &str = "users";
	const TREE_FEEDS: &str = "feeds";
	const TREE_ARTICLES: &str = "articles";

	pub fn new(cfg: &Config) -> Result<Arc<Self>> {
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
		let feeds = db.open_tree(Self::TREE_FEEDS)?;
		let articles = db.open_tree(Self::TREE_ARTICLES)?;

		let client = reqwest::ClientBuilder::new()
			.timeout(Duration::from_secs(20))
			.connect_timeout(Duration::from_secs(10))
			.build()?;

		let index_path = TempDir::new()?;

		let mut schema_builder = SchemaBuilder::new();
		let field_id = schema_builder.add_text_field("id", TEXT | FAST | STORED);
		let field_title = schema_builder.add_text_field("title", TEXT | FAST | STORED);
		let field_summary = schema_builder.add_text_field("summary", TEXT | FAST | STORED);
		let field_content = schema_builder.add_text_field("content", TEXT | FAST | STORED);

		let schema = schema_builder.build();
		let index = tantivy::Index::create_in_dir(&index_path, schema.clone())?;
		let index_writer = index.writer(50_000_000)?;
		let index_reader = index.reader()?;

		let searcher = Searcher {
			index_path,
			field_id,
			field_title,
			field_summary,
			field_content,
			schema,
			index,
			index_reader,
			index_writer: tokio::sync::Mutex::new(index_writer),
		};

		let app = Self {
			db,
			users,
			feeds,
			articles,
			client,
			searcher,
		};

		let arc = Arc::new(app);

		{
			let app = arc.clone();
			tokio::spawn(async move {
				let mut subscriber = app.articles.watch_prefix(b"");

				while let Some(evt) = (&mut subscriber).await {
					let index_writer = &mut app.searcher.index_writer.lock().await;
					match evt {
						sled::Event::Insert { key, value } => {
							let key = String::from_utf8_lossy(&key);

							// remove previous document
							let term = Term::from_field_text(field_id, &key);
							index_writer.delete_term(term);

							// add current document
							let article: Article = bincode::deserialize(&value)?;
							let doc = article.create_doc(&app);
							index_writer.add_document(doc)?;
						}
						sled::Event::Remove { key } => {
							let key = String::from_utf8_lossy(&key);

							let term = Term::from_field_text(field_id, &key);
							index_writer.delete_term(term);
						}
					}

					// commit changes
					index_writer.prepare_commit()?;
					index_writer.commit()?;
				}

				Ok::<(), Error>(())
			});
		}

		Ok(arc)
	}
}
