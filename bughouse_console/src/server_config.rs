#[derive(Debug, Clone)]
pub enum DatabaseOptions {
    NoDatabase,
    Sqlite(String),
    Postgres(String),
}

#[derive(Debug, Eq, PartialEq)]
pub enum AuthOptions {
    NoAuth,
    GoogleAuthFromEnv { callback_is_https: bool },
}

#[derive(Debug, Eq, PartialEq)]
pub enum SessionOptions {
    NoSessions,

    // Sessions terminate on server termination.
    WithNewRandomSecret,

    // Allows for sessions that survive server restart.
    // TODO: Support persistent sessions.
    #[allow(dead_code)]
    WithSecret(Vec<u8>),
}

#[derive(Debug, Clone)]
pub enum AllowedOrigin {
    Any,
    ThisSite(String),
}

#[derive(Debug)]
pub struct ServerConfig {
    pub database_options: DatabaseOptions,
    pub secret_database_options: DatabaseOptions,
    pub auth_options: AuthOptions,
    pub session_options: SessionOptions,
    pub static_content_url_prefix: String,
    pub allowed_origin: AllowedOrigin,
}
