use std::sync::Arc;

use http_types::StatusCode;

pub struct HttpServerStateImpl<DB> {
    pub google_auth: Option<crate::auth::GoogleAuth>,
    pub sessions_enabled: bool,
    pub auth_callback_is_https: bool,
    pub db: DB,
    pub static_content_url_prefix: String,
}

pub type HttpServerState<DB> = Arc<HttpServerStateImpl<DB>>;

impl<DB> crate::stats_handlers_tide::SuitableServerState for HttpServerState<DB>
    where DB: Sync + Send + 'static + crate::database::DatabaseReader
{
    type DB = DB;

    fn db(&self) -> &Self::DB {
        &self.db
    }

    fn static_content_url_prefix(&self) -> &str {
        &self.static_content_url_prefix
    }
}

// Non-panicking version of tide::Request::session()
pub fn get_session<DB>(
    req: &tide::Request<HttpServerState<DB>>,
) -> tide::Result<&tide::sessions::Session> {
    if req.state().sessions_enabled {
        Ok(req.session())
    } else {
        Err(tide::Error::from_str(
            StatusCode::NotImplemented,
            "Sessions are not enabled.",
        ))
    }
}

// Non-panicking version of tide::Request::session_mut()
pub fn get_session_mut<DB>(
    req: &mut tide::Request<HttpServerState<DB>>,
) -> tide::Result<&mut tide::sessions::Session> {
    if req.state().sessions_enabled {
        Ok(req.session_mut())
    } else {
        Err(tide::Error::from_str(
            StatusCode::NotImplemented,
            "Sessions are not enabled.",
        ))
    }
}
