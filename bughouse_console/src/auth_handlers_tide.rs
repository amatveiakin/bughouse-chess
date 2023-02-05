use tide::StatusCode;

use crate::http_server_state::*;
use crate::session::*;

pub const OAUTH_CSRF_COOKIE_NAME: &str = "oauth-csrf-state";

pub const AUTH_LOGIN_URL_PATH: &str = "/auth/login";
pub const AUTH_SESSION_URL_PATH: &str = "/auth/session";
pub const AUTH_LOGOUT_URL_PATH: &str = "/auth/logout";
pub const AUTH_MYSESSION_URL_PATH: &str = "/auth/mysession";

pub fn check_origin<T>(req: &tide::Request<T>) -> tide::Result<()> {
    let origin = req.header(http_types::headers::ORIGIN).map_or(
        Err(tide::Error::from_str(
            StatusCode::Forbidden,
            "Failed to get Origin header of the websocket request.",
        )),
        |origins| Ok(origins.last().as_str()),
    )?;

    // Derive the allowed origin from this request's URL.
    // For this to work, both the websocket endpoint and originating
    // web page need to be hosted on the same host and port.
    // If that changes, we'll need to check the Origin header against
    // an allow-list.
    if req.url().origin().ascii_serialization() != origin {
        if req.url().host() == Some(url::Host::Domain("localhost")) {
            let host = req.header(http_types::headers::HOST).map_or(
                Err(tide::Error::from_str(
                    StatusCode::Forbidden,
                    "Failed to get Host header of the localhost websocket request.",
                )),
                |origins| Ok(origins.last().as_str()),
            )?;
            if host == "localhost" || host.starts_with("localhost:") {
                return Ok(());
            }
            return Err(tide::Error::from_str(
                StatusCode::Forbidden,
                "Request to localhost from non-localhost origin.",
            ));
        }

        return Err(tide::Error::from_str(
            StatusCode::Forbidden,
            "Origin header on the websocket request does not match
                 the expected host",
        ));
    }
    Ok(())
}

// Initiates authentication process (e.g. with OAuth).
// TODO: this page should probably display some privacy considerations
//   and link to OAuth providers instead of just redirecting.
pub async fn handle_login(req: tide::Request<HttpServerState>) -> tide::Result {
    let mut callback_url = req.url().clone();
    callback_url.set_path(AUTH_SESSION_URL_PATH);
    if req.state().auth_callback_is_https {
        callback_url.set_scheme("https").map_err(|()| {
            anyhow::Error::msg(format!(
                "Failed to change URL scheme '{}' to 'https' for redirection.",
                callback_url.scheme()
            ))
        })?;
    }
    println!("{callback_url}");
    let (redirect_url, csrf_state) = req
        .state()
        .google_auth
        .as_ref()
        .ok_or(tide::Error::from_str(
            StatusCode::NotImplemented,
            "Google Auth is not enabled.",
        ))?
        .start(callback_url.into())?;

    let mut resp: tide::Response = req.into();
    resp.set_status(StatusCode::TemporaryRedirect);
    resp.insert_header(http_types::headers::LOCATION, redirect_url.as_str());

    // Using a separate cookie for oauth csrf state because the session
    // cookie has SameSite::Strict policy (and we want to keep that),
    // which prevents browsers from setting the session cookie upon
    // redirect.
    // This will use the default, which is SameSite::Lax on most browsers,
    // which should still be good enough.
    let mut csrf_cookie =
        http_types::cookies::Cookie::new(OAUTH_CSRF_COOKIE_NAME, csrf_state.secret().to_owned());
    csrf_cookie.set_http_only(true);
    resp.insert_cookie(csrf_cookie);
    Ok(resp)
}

// The "callback" handler of the authentication process.
// TODO: send the user to either the main page or their desider location.
//   HTTP redirect doesn't work because the session cookies do
//   not survive it, hence, some JS needs to be served that sends back
//   in 3...2...1... or something similar.
//   To send to the "desired location", pass the desired URL as a parameter
//   into /login and propagate it to callback_url.
pub async fn handle_session(mut req: tide::Request<HttpServerState>) -> tide::Result {
    let (auth_code, request_csrf_state) = req.query::<crate::auth::NewSessionQuery>()?.parse();
    let Some(oauth_csrf_state_cookie) = req.cookie(OAUTH_CSRF_COOKIE_NAME) else {
                return Err(tide::Error::from_str(
                    StatusCode::Forbidden, "Missing CSRF token cookie.",
                ));
            };
    if oauth_csrf_state_cookie.value() != request_csrf_state.secret() {
        return Err(tide::Error::from_str(
            StatusCode::Forbidden,
            "Non-matching CSRF token.",
        ));
    }

    let mut callback_url = req.url().clone();
    callback_url.set_query(Some(""));
    if req.state().auth_callback_is_https {
        callback_url.set_scheme("https").map_err(|()| {
            anyhow::Error::msg(format!(
                "Failed to change URL scheme '{}' to 'https' for redirection.",
                callback_url.scheme()
            ))
        })?;
    }
    let callback_url_str = callback_url.as_str().trim_end_matches('?').to_owned();
    println!("{callback_url_str}");

    let user_info = req
        .state()
        .google_auth
        .as_ref()
        .ok_or(tide::Error::from_str(
            StatusCode::NotImplemented,
            "Google auth is not enabled.",
        ))?
        .user_info(callback_url_str, auth_code)
        .await?;

    get_session_mut(&mut req)?.insert(
        "data",
        Session {
            logged_in: true,
            user_info: user_info.clone(),
        },
    )?;

    Ok(format!("You are now logged in. UserInfo: \n{:?}", user_info).into())
}

pub async fn handle_logout(mut req: tide::Request<HttpServerState>) -> tide::Result {
    get_session_mut(&mut req)?.remove("data");
    Ok("You are now logged out.".into())
}

pub async fn handle_mysession(req: tide::Request<HttpServerState>) -> tide::Result {
    match get_session(&req)?.get::<Session>("data") {
        None => Ok("You are not logged in.".into()),
        Some(Session { user_info, .. }) => {
            Ok(format!("You are logged in. UserInfo: {user_info:?}").into())
        }
    }
}
