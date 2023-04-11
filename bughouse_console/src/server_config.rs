use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DatabaseOptions {
    NoDatabase,
    Sqlite(String),
    Postgres(String),
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum StringSource {
    EnvVar(String),
    File(String),
}

impl StringSource {
    pub fn get(&self) -> anyhow::Result<String> {
        match self {
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

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SessionOptions {
    NoSessions,

    // Sessions terminate on server termination.
    WithNewRandomSecret,

    // Allows for sessions that survive server restart.
    // TODO: Support persistent sessions.
    #[allow(dead_code)]
    WithSecret(Vec<u8>),
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
