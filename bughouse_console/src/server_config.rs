use std::time::Duration;

use anyhow::Context;
use rand::Rng;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DatabaseOptions {
    NoDatabase,
    Sqlite(String),
    Postgres(String),
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum StringSource {
    Random { len: usize },
    Literal(String),
    EnvVar(String),
    File(String),
}

impl StringSource {
    pub fn get(&self) -> anyhow::Result<String> {
        match self {
            Self::Random { len } => Ok(rand::rng()
                .sample_iter(rand::distr::Uniform::new(0u8, 128u8).unwrap())
                .take(*len)
                .map(|b: u8| -> char { b.into() })
                .collect()),
            Self::Literal(s) => Ok(s.clone()),
            Self::EnvVar(v) => {
                std::env::var(v).context(format!("Missing environment variable '{v}'."))
            }
            Self::File(f) => {
                std::fs::read_to_string(f).context(format!("Failed to read file '{f}'."))
            }
        }
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AuthOptions {
    pub callback_is_https: bool,
    pub google: Option<GoogleAuthOptions>,
    pub lichess: Option<LichessAuthOptions>,
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GoogleAuthOptions {
    pub client_id_source: StringSource,
    pub client_secret_source: StringSource,
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LichessAuthOptions {
    pub client_id_source: StringSource,
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize, Clone)]
pub enum SessionOptions {
    NoSessions,

    WithSessions {
        // When the secret is preserved, client-side sessions survive server
        // restarts. When Random is used, or the secret changes,
        // the sessions are terminated.
        secret: StringSource,
        #[serde(with = "humantime_serde")]
        expire_in: Duration,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AllowedOrigin {
    Any,
    ThisSite(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerConfig {
    pub database_options: DatabaseOptions,
    pub secret_database_options: DatabaseOptions,
    pub auth_options: Option<AuthOptions>,
    pub session_options: SessionOptions,
    pub static_content_url_prefix: String,
    pub allowed_origin: AllowedOrigin,
    pub check_git_version: bool,
    #[serde(with = "humantime_serde")]
    pub max_starting_time: Option<Duration>,
}
