#![forbid(unsafe_code)]

mod app;
mod db;
mod err;
mod fetch;

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use app::App;
use axum::{
	extract::{Query, State},
	http::Request,
	middleware::Next,
	response::Response,
	routing::{any, get, post},
	Extension, Json, Router,
};
use base64::Engine;
use db::{Article, ExportOpts, Feed, NewFeed, NewUser, PatchFeed, User};
pub use err::{Error, Result};

use serde::Deserialize;
use tower_http::cors::CorsLayer;

#[tokio::main]
async fn main() {
	match main2().await {
		Ok(_) => (),
		Err(e) => eprintln!("{}", e),
	}
}

type AppState = Arc<App>;

#[derive(Clone)]
pub struct CurrentUser(String);

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

	req.extensions_mut().insert(CurrentUser(user.username));
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
	let cfg = app::Config {
		db_path: root.join("db.sled"),
	};
	let app = App::new(&cfg)?;

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

	// init routes
	let state = Arc::new(app);

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
		.route_layer(axum::middleware::from_fn_with_state(state.clone(), auth))
		.with_state(state.clone())
		.layer(CorsLayer::permissive());

	let addr = SocketAddr::new(addr.parse().unwrap(), port.parse().unwrap());
	axum::Server::bind(&addr)
		.serve(router.into_make_service())
		.await
		.unwrap();

	Ok(())
}

async fn get_feeds(
	State(state): State<AppState>,
	Extension(CurrentUser(username)): Extension<CurrentUser>,
) -> Result<Json<Vec<Feed>>> {
	Feed::get_all(&state.open_user(&username)?).map(Json)
}

async fn post_feed(
	State(state): State<AppState>,
	Extension(CurrentUser(username)): Extension<CurrentUser>,
	Json(new_feed): Json<NewFeed>,
) -> Result<()> {
	new_feed
		.insert(&state.open_user(&username)?)
		.await
		.map(|_| ())
}

async fn patch_feed(
	State(state): State<AppState>,
	Extension(CurrentUser(username)): Extension<CurrentUser>,
	Json(patch_feed): Json<PatchFeed>,
) -> Result<()> {
	patch_feed.apply(&state.open_user(&username)?)
}

async fn refresh(
	State(state): State<AppState>,
	Extension(CurrentUser(username)): Extension<CurrentUser>,
) -> Result<()> {
	fetch::fetch_all_feeds(&state.open_user(&username)?).await
}

async fn get_articles(
	State(state): State<AppState>,
	Extension(CurrentUser(username)): Extension<CurrentUser>,
) -> Result<Json<Vec<Article>>> {
	Article::get_all(&state.open_user(&username)?).map(Json)
}

async fn import(
	State(state): State<AppState>,
	Extension(CurrentUser(username)): Extension<CurrentUser>,
	body: String,
) -> Result<()> {
	let opml = opml::OPML::from_str(&body)?;
	db::import(&state.open_user(&username)?, db::ImportOpts::Opml(opml)).await
}

async fn export(
	State(state): State<AppState>,
	Extension(CurrentUser(username)): Extension<CurrentUser>,
	Query(opts): Query<ExportOpts>,
) -> Result<String> {
	db::export(&state.open_user(&username)?, opts)
}

#[derive(Deserialize)]
struct SearchQuery {
	q: String,
}

async fn search(
	State(state): State<AppState>,
	Extension(CurrentUser(username)): Extension<CurrentUser>,
	Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<String>>> {
	state.open_user(&username)?.search(&query.q).map(Json)
}
