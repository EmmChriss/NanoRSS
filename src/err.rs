pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
	#[error("failed to determine nanorss directory")]
	NoRootDir,

	#[error("username already taken")]
	UsernameTaken,

	#[error("username not found")]
	UsernameNotFound,

	#[error("password incorrect")]
	PasswordIncorrect,

	#[error("failed to hash password: {0}")]
	Bcrypt(#[from] bcrypt::BcryptError),

	#[error("database error: {0}")]
	Sled(#[from] sled::Error),

	#[error("serialization error: {0}")]
	RmpEncode(#[from] bincode::Error),
}
