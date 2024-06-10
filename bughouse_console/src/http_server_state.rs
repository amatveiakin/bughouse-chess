use std::sync::{Arc, Mutex};

use bughouse_chess::server;
use bughouse_chess::session_store::{SessionId, SessionStore};
use http_types::StatusCode;
use url::Url;

use crate::secret_persistence::SecretDatabaseRW;

pub struct HttpServerStateImpl<DB> {
    pub google_auth: Option<crate::auth::GoogleAuth>,
    pub lichess_auth: Option<crate::auth::LichessAuth>,
    pub sessions_enabled: bool,
    pub auth_callback_is_https: bool,
    pub db: DB,
    pub secret_db: Box<dyn SecretDatabaseRW>,
    pub static_content_url_prefix: String,
    pub session_store: Arc<Mutex<SessionStore>>,
    pub server_info: Arc<Mutex<server::ServerInfo>>,
}

pub type HttpServerState<DB> = Arc<HttpServerStateImpl<DB>>;

impl<DB> crate::stats_handlers_tide::SuitableServerState for HttpServerState<DB>
where
    DB: Sync + Send + 'static + crate::persistence::DatabaseReader,
{
    type DB = DB;

    fn db(&self) -> &Self::DB { &self.db }

    fn static_content_url_prefix(&self) -> &str { &self.static_content_url_prefix }
}

impl<DB> HttpServerStateImpl<DB> {
    pub fn upgrade_auth_callback(&self, callback: &mut Url) -> tide::Result<()> {
        if self.auth_callback_is_https {
            callback.set_scheme("https").map_err(|()| {
                anyhow::Error::msg(format!(
                    "Failed to change URL scheme '{}' to 'https' for redirection.",
                    callback.scheme()
                ))
            })?;
        }
        Ok(())
    }
}

// Non-panicking version of tide::Request::session().id()
pub fn get_session_id<DB>(req: &tide::Request<HttpServerState<DB>>) -> tide::Result<SessionId> {
    get_session(req).map(|s| SessionId::new(s.id().to_owned()))
}

// Non-panicking version of tide::Request::session()
fn get_session<DB>(
    req: &tide::Request<HttpServerState<DB>>,
) -> tide::Result<&tide::sessions::Session> {
    if req.state().sessions_enabled {
        Ok(req.session())
    } else {
        Err(tide::Error::from_str(StatusCode::NotImplemented, "Sessions are not enabled."))
    }
}
