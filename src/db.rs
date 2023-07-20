use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

const TREE_USERS: &str = "users";
const TREE_FEEDS: &str = "feeds";
const TREE_ARTICLES: &str = "articles";

pub struct Db {
	db: sled::Db,
	users: sled::Tree,
	feeds: sled::Tree,
	articles: sled::Tree,
}

impl Db {
	pub fn create_or_open(path: impl AsRef<Path>) -> Result<Self> {
		let db = sled::Config::default()
			.path(path)
			.flush_every_ms(Some(1000));

		let db = if cfg!(debug_assertions) {
			db.print_profile_on_drop(true)
		} else {
			db
		};

		let db = db.open()?;
		let users = db.open_tree(TREE_USERS)?;
		let feeds = db.open_tree(TREE_FEEDS)?;
		let articles = db.open_tree(TREE_ARTICLES)?;

		Ok(Db {
			db,
			users,
			feeds,
			articles,
		})
	}
}

#[derive(Serialize, Deserialize)]
pub struct NewUser {
	pub username: String,
	pub password: String,
}

impl NewUser {
	pub fn insert(self, db: &Db) -> Result<User> {
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

#[derive(Serialize, Deserialize, Clone)]
pub struct User {
	pub username: String,
	pub pass_hash: String,
}

impl User {
	fn get_user(db: &Db, username: &str) -> Result<Option<User>> {
		db.users
			.get(username.as_bytes())?
			.map(|bytes| bincode::deserialize(&bytes))
			.transpose()
			.map_err(Into::into)
	}

	pub fn try_login(db: &Db, username: &str, password: &str) -> Result<User> {
		let user = Self::get_user(db, username)?.ok_or(Error::UsernameNotFound)?;

		// validate password
		if !bcrypt::verify(password, &user.pass_hash)? {
			return Err(Error::PasswordIncorrect);
		}

		Ok(user)
	}
}

#[derive(Serialize, Deserialize)]
pub struct Feed {}

#[derive(Serialize, Deserialize)]
pub struct Article {}
