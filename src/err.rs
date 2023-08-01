use axum::http::StatusCode;
use axum::response::IntoResponse;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum Error {
	#[error("failed to determine nanorss directory")]
	NoRootDir,

	#[error("username already taken")]
	UsernameTaken,

	#[error("username not found")]
	UsernameNotFound,

	#[error("password incorrect")]
	PasswordIncorrect,

	#[error("{0} was not found")]
	NotFound(String),

	#[error("failed to hash password: {0}")]
	Bcrypt(#[from] bcrypt::BcryptError),

	#[error("database error: {0}")]
	Sled(#[from] sled::Error),

	#[error("serialization error: {0}")]
	Encode(#[from] bincode::Error),

	#[error("http client error: {0}")]
	Reqwest(#[from] reqwest::Error),

	#[error("error while parsing feed: {0}")]
	FeedRS(#[from] feed_rs::parser::ParseFeedError),

	#[error("error while parsing base64 string: {0}")]
	Base64(#[from] base64::DecodeError),

	#[error("error parsing opml: {0}")]
	Opml(#[from] opml::Error),

	#[error("error parsing url: {0}")]
	Url(#[from] url::ParseError),

	#[error("io error: {0}")]
	Io(#[from] std::io::Error),

	#[error("string not utf8: {0}")]
	Utf8(#[from] std::string::FromUtf8Error),

	#[error("transaction error: {0}")]
	Transaction(#[from] sled::transaction::TransactionError),
}

impl IntoResponse for Error {
	fn into_response(self) -> axum::response::Response {
		match self {
			Error::UsernameTaken => {
				(StatusCode::BAD_REQUEST, "Username already taken").into_response()
			}
			Error::UsernameNotFound | Error::PasswordIncorrect => {
				(StatusCode::UNAUTHORIZED, "Username or password incorrect").into_response()
			}
			_ => (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", self)).into_response(),
		}
	}
}
