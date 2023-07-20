#![forbid(unsafe_code)]

mod db;
mod err;

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{
	extract::State,
	http::Request,
	middleware::Next,
	response::Response,
	routing::{get, post},
	Router,
};

use db::{Db, NewUser, User};
pub use err::{Error, Result};

use tower_http::cors::CorsLayer;

#[tokio::main]
async fn main() {
	match main2().await {
		Ok(_) => (),
		Err(e) => eprintln!("{}", e),
	}
}

struct App {
	db: Db,
}

type AppState = Arc<tokio::sync::RwLock<App>>;

async fn get_feeds() -> String {
	todo!()
}

pub struct CurrentUser(User);

async fn auth<B>(
	State(state): State<AppState>,
	mut req: Request<B>,
	next: Next<B>,
) -> Result<Response, axum::http::StatusCode> {
	let auth_header = req
		.headers()
		.get(axum::http::header::AUTHORIZATION)
		.and_then(|header| header.to_str().ok());

	let basic = match auth_header {
		None => return Err(axum::http::StatusCode::UNAUTHORIZED),
		Some(bearer) => bearer,
	};

	let mut split = basic.split(':');
	let (username, password) = match (split.next(), split.next()) {
		(Some(a), Some(b)) => (a, b),
		_ => return Err(axum::http::StatusCode::UNAUTHORIZED),
	};

	let user = match User::try_login(&state.read_owned().await.db, username, password) {
		Ok(user) => user,
		_ => return Err(axum::http::StatusCode::UNAUTHORIZED),
	};

	req.extensions_mut().insert(CurrentUser(user));
	Ok(next.run(req).await)
}

async fn main2() -> Result<()> {
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
	let username = dotenvy::var("USER");
	let password = dotenvy::var("PASSWD");

	// init logger
	env_logger::init();

	// init and seed database
	let db = Db::create_or_open(root.join("db.sled"))?;

	match (username, password) {
		(Ok(username), Ok(password)) => {
			let new_user = NewUser { username, password };
			new_user.insert(&db)?;
		}
		(Err(_), Err(_)) => (),
		(Err(_), _) | (_, Err(_)) => {
			log::error!("both USER and PASSWD need to be set to create a user")
		}
	}

	// init state
	let state = Arc::new(tokio::sync::RwLock::new(App { db }));

	// init routes
	let router = Router::new()
		.route("/api/v1/login", post(|| async { "".to_string() }))
		.route("/api/v1/import", post(|| async { "".to_string() }))
		.route("/api/v1/export", post(|| async { "".to_string() }))
		.route("/api/v1/news", get(|| async { "".to_string() }))
		.route("/api/v1/feeds", get(|| async { "".to_string() }))
		.route("/api/v1/articles", get(|| async { "".to_string() }))
		.route("/api/v1/search", post(|| async { "".to_string() }))
		.route("/api/v1/refresh", get(|| async { "".to_string() }))
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
