#![forbid(unsafe_code)]

mod db;
mod err;
mod fetch;

use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use axum::{
	extract::{Query, State},
	http::Request,
	middleware::Next,
	response::Response,
	routing::{any, get, post},
	Json, Router,
};
use base64::Engine;
use db::{Article, ExportOpts, Feed, NewFeed, NewUser, PatchFeed, User};
pub use err::{Error, Result};

use serde::Deserialize;
use tantivy::{collector::TopDocs, query::QueryParser, schema::*};
use tempfile::TempDir;
use tower_http::cors::CorsLayer;

#[tokio::main]
async fn main() {
	match main2().await {
		Ok(_) => (),
		Err(e) => eprintln!("{}", e),
	}
}

pub struct Config {
	db_path: PathBuf,
}

pub struct Searcher {
	// tantivy index schema
	field_id: tantivy::schema::Field,
	field_title: tantivy::schema::Field,
	field_summary: tantivy::schema::Field,
	field_content: tantivy::schema::Field,

	// search setup
	index_path: TempDir,
	schema: tantivy::schema::Schema,
	index: tantivy::Index,
	index_writer: tokio::sync::Mutex<tantivy::IndexWriter>,
	index_reader: tantivy::IndexReader,
}

#[derive(Deserialize)]
pub struct SearchQuery {
	text: String,
	order_by: OrderBy,
	order: Order,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Order {
	Asc,
	Desc,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderBy {
	Title,
}

impl Searcher {
	pub fn search(&self, query_request: SearchQuery) -> Vec<Document> {
		let searcher = self.index_reader.searcher();
		let query_parser = QueryParser::for_index(
			&self.index,
			vec![self.field_title, self.field_summary, self.field_content],
		);

		let query = query_parser.parse_query(&query_request.text).unwrap();

		let top_docs = searcher.search(&query, &TopDocs::with_limit(10)).unwrap();

		let mut vec = Vec::new();
		for (_score, doc_address) in top_docs {
			let retrieved_doc = searcher.doc(doc_address).unwrap();
			vec.push(retrieved_doc);
		}

		vec
	}
}

pub struct App {
	db: sled::Db,
	users: sled::Tree,
	feeds: sled::Tree,
	articles: sled::Tree,
	client: reqwest::Client,
	searcher: Searcher,
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

type AppState = Arc<App>;

pub struct CurrentUser(User);

async fn auth<B>(
	State(state): State<AppState>,
	mut req: Request<B>,
	next: Next<B>,
) -> Result<Response, Error> {
	let auth_header = req
		.headers()
		.get(axum::http::header::AUTHORIZATION)
		.and_then(|header| header.to_str().ok());

	let auth = auth_header.ok_or(Error::UsernameNotFound)?;

	let mut split = auth.split(" ");
	let (kind, payload) = match (split.next(), split.next()) {
		(Some(a), Some(b)) => (a, b),
		_ => return Err(Error::UsernameNotFound),
	};

	let user = match kind {
		"Basic" => {
			let decoded_bytes = base64::engine::general_purpose::STANDARD_NO_PAD.decode(payload)?;
			let decoded = String::from_utf8(decoded_bytes).map_err(|_| Error::UsernameNotFound)?;

			let mut split = decoded.split(':');
			let (username, password) = match (split.next(), split.next()) {
				(Some(a), Some(b)) => (a, b),
				_ => return Err(Error::UsernameNotFound),
			};

			User::try_login(&state, username, password)?
		}
		_ => unimplemented!(),
	};

	req.extensions_mut().insert(CurrentUser(user));
	Ok(next.run(req).await)
}

async fn main2() -> anyhow::Result<()> {
	// get environment, crash if missing
	let addr = dotenvy::var("ADDRESS").unwrap_or("0.0.0.0".into());
	let port = dotenvy::var("PORT").unwrap_or("8888".into());
	let root = dotenvy::var("DATA_PATH")
		.ok()
		.map(PathBuf::from)
		.or_else(|| {
			dirs::data_dir().map(|mut p| {
				p.push("nanorss");
				p
			})
		})
		.ok_or(Error::NoRootDir)?;
	let username = dotenvy::var("USERNAME");
	let password = dotenvy::var("PASSWORD");

	// init logger
	env_logger::init();

	// init and seed db
	let cfg = Config {
		db_path: root.join("db.sled"),
	};
	let app = App::new(&cfg)?;
	app.articles.clear().unwrap();
	app.feeds.clear().unwrap();

	match (username, password) {
		(Ok(username), Ok(password)) => {
			let new_user = NewUser { username, password };
			new_user
				.insert(&app)
				.map(|u| log::info!("created user {}", u.username))
				.unwrap_or_else(|e| log::warn!("could not create user: {}", e));
		}
		(Err(_), Err(_)) => (),
		(Err(_), _) | (_, Err(_)) => {
			log::error!("both USER and PASSWD need to be set to create a user")
		}
	}

	// insert some dummy feeds
	db::NewFeed {
		url: url::Url::parse("https://without.boats/blog/index.xml")?,
		name: Some("Without Boats".into()),
		scraper: None,
	}
	.insert(&app)
	.await?;

	db::NewFeed {
		url: url::Url::parse("https://fasterthanli.me/index.xml")?,
		name: Some("Faster Than Lime".into()),
		scraper: None,
	}
	.insert(&app)
	.await?;

	// init routes
	let router = Router::new()
		.route("/api/v1/status", any(|| async { "OK".to_string() }))
		.route("/api/v1/import", post(import))
		.route("/api/v1/export", post(export))
		.route(
			"/api/v1/feeds",
			get(get_feeds).post(post_feed).patch(patch_feed),
		)
		.route("/api/v1/articles", get(get_articles))
		.route("/api/v1/search", post(search))
		.route("/api/v1/refresh", post(refresh))
		.route_layer(axum::middleware::from_fn_with_state(app.clone(), auth))
		.with_state(app.clone())
		.layer(CorsLayer::permissive());

	let addr = SocketAddr::new(addr.parse().unwrap(), port.parse().unwrap());
	axum::Server::bind(&addr)
		.serve(router.into_make_service())
		.await
		.unwrap();

	Ok(())
}

async fn get_feeds(State(state): State<AppState>) -> Result<Json<Vec<Feed>>> {
	Feed::get_all(&state).map(Json)
}

async fn post_feed(State(state): State<AppState>, Json(new_feed): Json<NewFeed>) -> Result<()> {
	new_feed.insert(&state).await.map(|_| ())
}

async fn patch_feed(
	State(state): State<AppState>,
	Json(patch_feed): Json<PatchFeed>,
) -> Result<()> {
	patch_feed.apply(&state)
}

async fn refresh(State(state): State<AppState>) -> Result<()> {
	fetch::fetch_all_feeds(&state).await
}

async fn get_articles(State(state): State<AppState>) -> Result<Json<Vec<Article>>> {
	Article::get_all(&state).map(Json)
}

async fn import(State(state): State<AppState>, body: String) -> Result<()> {
	let opml = opml::OPML::from_str(&body)?;
	db::import(&state, db::ImportOpts::Opml(opml)).await
}

async fn export(State(state): State<AppState>, Query(opts): Query<ExportOpts>) -> Result<String> {
	db::export(&state, opts)
}

async fn search(
	State(state): State<AppState>,
	Query(query): Query<SearchQuery>,
) -> impl axum::response::IntoResponse {
	Json(state.searcher.search(query))
}
