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
            Self::Random { len } => Ok(rand::thread_rng()
                .sample_iter(rand::distributions::Uniform::new(0, 128))
                .take(*len)
                .map(|b: u8| -> char { b.try_into().unwrap() })
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
pub enum AuthOptions {
    NoAuth,
    Google {
        callback_is_https: bool,
        client_id_source: StringSource,
        client_secret_source: StringSource,
    },
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize, Clone)]
pub enum SessionOptions {
    NoSessions,

    WithSessions {
        // When the secret is preserved, client-side sessions survive server
        // restarts. When Random is used, or the secret changes,
        // the sessions are terminated.
        secret: StringSource,
        expire_in: std::time::Duration,
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
    pub auth_options: AuthOptions,
    pub session_options: SessionOptions,
    pub static_content_url_prefix: String,
    pub allowed_origin: AllowedOrigin,
}
