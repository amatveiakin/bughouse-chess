use std::sync::Arc;

use http_types::StatusCode;

pub struct HttpServerStateImpl {
    pub google_auth: Option<crate::auth::GoogleAuth>,
    pub sessions_enabled: bool,
    pub auth_callback_is_https: bool,
}

pub type HttpServerState = Arc<HttpServerStateImpl>;

// Non-panicking version of tide::Request::session()
pub fn get_session(req: &tide::Request<HttpServerState>) -> tide::Result<&tide::sessions::Session> {
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
pub fn get_session_mut(
    req: &mut tide::Request<HttpServerState>,
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
