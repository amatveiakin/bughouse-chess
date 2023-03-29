use anyhow::anyhow;
use oauth2::AuthorizationCode;
use serde::Deserialize;
use tide::StatusCode;
use time::OffsetDateTime;

use bughouse_chess::session::{GoogleOAuthRegistrationInfo, RegistrationMethod, Session, UserInfo};

use crate::auth;
use crate::http_server_state::*;
use crate::prod_server_helpers::validate_player_name;
use crate::secret_persistence::DeletedAccount;
use crate::secret_persistence::{Account, LiveAccount};
use crate::server_config::AllowedOrigin;

pub const OAUTH_CSRF_COOKIE_NAME: &str = "oauth-csrf-state";

pub const AUTH_SIGNUP_PATH: &str = "/auth/signup";
pub const AUTH_LOGIN_PATH: &str = "/auth/login";
pub const AUTH_LOGOUT_PATH: &str = "/auth/logout";
pub const AUTH_SIGN_WITH_GOOGLE_PATH: &str = "/auth/sign-with-google";
pub const AUTH_CONTINUE_SIGN_WITH_GOOGLE_PATH: &str = "/auth/continue-sign-with-google";
pub const AUTH_FINISH_SIGNUP_WITH_GOOGLE_PATH: &str = "/auth/finish-signup-with-google";
pub const AUTH_CHANGE_ACCOUNT_PATH: &str = "/auth/change-account";
pub const AUTH_DELETE_ACCOUNT_PATH: &str = "/auth/delete-account";
pub const AUTH_MYSESSION_PATH: &str = "/auth/mysession";

#[derive(Deserialize)]
struct SignupData {
    user_name: String,
    email: String,  // optional; empty string means none
    password: String,
}

#[derive(Deserialize)]
struct LoginData {
    user_name: String,
    password: String,
}

#[derive(Deserialize)]
struct FinishSignupWithGoogleData {
    user_name: String,
}

#[derive(Deserialize)]
struct ChangeAccountData {
    current_password: String,      // must be present to authorize any changes
    email: Option<String>,         // empty string means remove / don't add
    new_password: Option<String>,  // empty string means keep old password
}

#[derive(Deserialize)]
struct DeleteAccountData {
    password: Option<String>,  // must be present to authorize deletion
}

pub fn check_origin<T>(req: &tide::Request<T>, allowed_origin: &AllowedOrigin) -> tide::Result<()> {
    let allowed_origin = match allowed_origin {
        AllowedOrigin::Any => return Ok(()),
        AllowedOrigin::ThisSite(o) => o,
    };
    let origin = req.header(http_types::headers::ORIGIN).map_or(
        Err(tide::Error::from_str(
            StatusCode::Forbidden,
            "Failed to get Origin header of the websocket request.",
        )),
        |origins| Ok(origins.last().as_str()),
    )?;

    if origin == allowed_origin {
        return Ok(());
    }
    Err(tide::Error::from_str(
        StatusCode::Forbidden,
        "Origin of the websocket request does not match the expected value.",
    ))
}

pub fn check_google_csrf<T>(req: &tide::Request<T>) -> tide::Result<AuthorizationCode> {
    let (auth_code, request_csrf_state) = req.query::<auth::NewSessionQuery>()?.parse();
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
    Ok(auth_code)
}

pub fn authorize_access_by_password(password: &str, account: &LiveAccount) -> anyhow::Result<()> {
    // Improvement potential: Distinguish between Forbidden and InternalServerError.
    if account.registration_method != RegistrationMethod::Password {
        return Err(anyhow!(
            "Cannot log in: {} authentification method was used during sign-up.",
            account.registration_method.to_string()
        ));
    }
    let Some(password_hash) = &account.password_hash else {
        return Err(anyhow!("Cannot verify password."));
    };
    // TODO: Update password hash on login:
    //   https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html#upgrading-the-work-factor
    let password_ok = auth::verify_password(&password, &password_hash)?;
    if !password_ok {
        return Err(anyhow!("Invalid password."));
    }
    Ok(())
}


pub async fn handle_signup<DB: Send + Sync + 'static>(
    mut req: tide::Request<HttpServerState<DB>>,
) -> tide::Result {
    let SignupData{ user_name, email, password } = req.body_form().await?;
    let email = if email.is_empty() { None } else { Some(email) };

    validate_player_name(&user_name)
        .map_err(|err| tide::Error::from_str(StatusCode::Forbidden, err))?;

    let existing_account = req.state().secret_db.account_by_user_name(&user_name).await?;
    if existing_account.is_some() {
        return Err(tide::Error::from_str(
            StatusCode::Forbidden,
            format!("Username '{}' is already taken.", &user_name),
        ));
    };

    let password_hash = auth::hash_password(&password)
        .map_err(|err| tide::Error::new(StatusCode::InternalServerError, err))?;

    // TODO: Confirm email if not empty.
    req.state().secret_db.create_account(
        user_name.clone(),
        email.clone(),
        Some(password_hash),
        RegistrationMethod::Password,
        OffsetDateTime::now_utc(),
    ).await.map_err(|err| tide::Error::new(StatusCode::InternalServerError, err))?;

    let session = Session::LoggedIn(UserInfo {
        user_name,
        email,
        registration_method: RegistrationMethod::Password,
    });
    let session_id = get_session_id(&req)?;
    req.state().session_store.lock().unwrap().set(session_id, session);

    let mut resp: tide::Response = req.into();
    resp.set_status(StatusCode::Ok);
    Ok(resp)
}

pub async fn handle_login<DB: Send + Sync + 'static>(
    mut req: tide::Request<HttpServerState<DB>>,
) -> tide::Result {
    let form_data: LoginData = req.body_form().await?;
    let account = req.state().secret_db.account_by_user_name(&form_data.user_name).await?;
    let Some(Account::Live(account)) = account else {
        return Err(tide::Error::from_str(
            StatusCode::Forbidden,
            format!("User '{}' does not exist.", &form_data.user_name),
        ));
    };

    authorize_access_by_password(&form_data.password, &account)
        .map_err(|err| tide::Error::new(StatusCode::Forbidden, err))?;

    let session = Session::LoggedIn(UserInfo {
        user_name: account.user_name,
        email: account.email,
        registration_method: RegistrationMethod::Password,
    });
    let session_id = get_session_id(&req)?;
    req.state().session_store.lock().unwrap().set(session_id, session);

    let mut resp: tide::Response = req.into();
    resp.set_status(StatusCode::Ok);
    Ok(resp)
}

// Initiates authentication process (e.g. with OAuth).
// TODO: this page should probably display some privacy considerations
//   and link to OAuth providers instead of just redirecting.
pub async fn handle_sign_with_google<DB: Send + Sync + 'static>(
    req: tide::Request<HttpServerState<DB>>,
) -> tide::Result {
    let mut callback_url = req.url().clone();
    callback_url.set_path(AUTH_CONTINUE_SIGN_WITH_GOOGLE_PATH);
    req.state().upgrade_auth_callback(&mut callback_url)?;
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

    // Original motivation:
    // Using a separate cookie for oauth csrf state because the session
    // cookie has SameSite::Strict policy (and we want to keep that),
    // which prevents browsers from setting the session cookie upon
    // redirect.
    // This will use the default, which is SameSite::Lax on most browsers,
    // which should still be good enough.
    // Update: this is no longer the case.
    // TODO: move csrf_toke into tide's session.
    let mut csrf_cookie =
        http_types::cookies::Cookie::new(OAUTH_CSRF_COOKIE_NAME, csrf_state.secret().to_owned());
    csrf_cookie.set_http_only(true);
    resp.insert_cookie(csrf_cookie);
    Ok(resp)
}

// The "callback" handler of the Google authentication process.
// TODO: send the user to either the main page or their desider location.
//   HTTP redirect doesn't work because the session cookies do
//   not survive it, hence, some JS needs to be served that sends back
//   in 3...2...1... or something similar.
//   To send to the "desired location", pass the desired URL as a parameter
//   into /sign-with-google and propagate it to callback_url.
pub async fn handle_continue_sign_with_google<DB: Send + Sync + 'static>(
    req: tide::Request<HttpServerState<DB>>,
) -> tide::Result {
    let auth_code = check_google_csrf(&req)?;

    let mut callback_url = req.url().clone();
    callback_url.set_query(Some(""));
    req.state().upgrade_auth_callback(&mut callback_url)?;
    let callback_url_str = callback_url.as_str().trim_end_matches('?').to_owned();

    let email = req
        .state()
        .google_auth
        .as_ref()
        .ok_or(tide::Error::from_str(
            StatusCode::NotImplemented,
            "Google auth is not enabled.",
        ))?
        .email(callback_url_str, auth_code)
        .await?;

    let account = req.state().secret_db.account_by_email(&email).await?;

    let session = match account {
        None => Session::GoogleOAuthRegistering(GoogleOAuthRegistrationInfo { email }),
        Some(Account::Live(account)) => {
            if account.registration_method != RegistrationMethod::GoogleOAuth {
                return Err(tide::Error::from_str(
                    StatusCode::Forbidden,
                    format!(
                        "Cannot log in with Google: {} authentification method was used during sign-up.",
                        account.registration_method.to_string()
                    )
                ));
            }
            Session::LoggedIn(UserInfo {
                user_name: account.user_name,
                email: Some(email),
                registration_method: RegistrationMethod::GoogleOAuth,
            })
        },
        Some(Account::Deleted(_)) => {
            // Should not happen: deleted accounts don't have emails.
            return Err(tide::Error::from_str(
                StatusCode::Forbidden,
                "Cannot log in with Google: no such account"
            ));
        },
    };

    let session_id = get_session_id(&req)?;
    req.state().session_store.lock().unwrap().set(session_id, session);

    let mut resp: tide::Response = req.into();
    resp.set_status(StatusCode::TemporaryRedirect);
    resp.insert_header(http_types::headers::LOCATION, "/");
    Ok(resp)
}

pub async fn handle_finish_signup_with_google<DB: Send + Sync + 'static>(
    mut req: tide::Request<HttpServerState<DB>>,
) -> tide::Result {
    let FinishSignupWithGoogleData{ user_name } = req.body_form().await?;

    validate_player_name(&user_name)
        .map_err(|err| tide::Error::from_str(StatusCode::Forbidden, err))?;

    let existing_account = req.state().secret_db.account_by_user_name(&user_name).await?;
    if existing_account.is_some() {
        return Err(tide::Error::from_str(
            StatusCode::Forbidden,
            format!("Username '{}' is already taken.", &user_name),
        ));
    };

    let session_id = get_session_id(&req)?;
    let email = {
        let session_store = req.state().session_store.lock().unwrap();
        match session_store.get(&session_id) {
            Some(Session::GoogleOAuthRegistering(GoogleOAuthRegistrationInfo{ email })) => email.clone(),
            _ => {
                return Err(tide::Error::from_str(
                    StatusCode::Forbidden,
                    format!("Error creating account with Google.")
                ));
            }
        }
    };

    req.state().secret_db.create_account(
        user_name.clone(),
        Some(email.clone()),
        None,
        RegistrationMethod::GoogleOAuth,
        OffsetDateTime::now_utc(),
    ).await.map_err(|err| tide::Error::new(StatusCode::InternalServerError, err))?;

    let session = Session::LoggedIn(UserInfo {
        user_name,
        email: Some(email),
        registration_method: RegistrationMethod::GoogleOAuth,
    });
    req.state().session_store.lock().unwrap().set(session_id, session);

    let mut resp: tide::Response = req.into();
    resp.set_status(StatusCode::Ok);
    Ok(resp)
}

pub async fn handle_logout<DB>(req: tide::Request<HttpServerState<DB>>) -> tide::Result {
    let session_id = get_session_id(&req)?;
    req.state().session_store.lock().unwrap()
        .update_if_exists(&session_id, Session::logout);
    Ok("You are now logged out.".into())
}

pub async fn handle_change_account<DB: Send + Sync + 'static>(
    mut req: tide::Request<HttpServerState<DB>>,
) -> tide::Result {
    let ChangeAccountData{ current_password, email, new_password } = req.body_form().await?;
    let email = email.filter(|s| !s.is_empty());
    let email_copy = email.clone();
    let new_password = new_password.filter(|s| !s.is_empty());

    let session_id = get_session_id(&req)?;
    let user_name = {
        let session_store = req.state().session_store.lock().unwrap();
        match session_store.get(&session_id) {
            Some(Session::LoggedIn(UserInfo{ user_name, .. })) => user_name.clone(),
            _ => {
                return Err(tide::Error::from_str(
                    StatusCode::Forbidden,
                    format!("You need to log in in order to change account settings.")
                ));
            }
        }
    };

    let old_account = req.state().secret_db.account_by_user_name(&user_name)
        .await?
        .and_then(Account::live)
        .ok_or(tide::Error::from_str(
            StatusCode::Forbidden,
            format!("User '{}' not found.", user_name),
        ))?;

    req.state().secret_db.update_account_txn(old_account.id, Box::new(move |account| -> anyhow::Result<()> {
        if account.registration_method == RegistrationMethod::Password {
            authorize_access_by_password(&current_password, &account)?;
        }
        account.email = email;
        if let Some(new_password) = new_password {
            account.password_hash = Some(auth::hash_password(&new_password)?);
        }
        Ok(())
    })).await.map_err(|err| tide::Error::new(StatusCode::Forbidden, err))?;

    let session = Session::LoggedIn(UserInfo {
        user_name,
        email: email_copy,
        registration_method: old_account.registration_method,
    });
    req.state().session_store.lock().unwrap().set(session_id, session);

    let mut resp: tide::Response = req.into();
    resp.set_status(StatusCode::Ok);
    Ok(resp)
}

pub async fn handle_delete_account<DB: Send + Sync + 'static>(
    mut req: tide::Request<HttpServerState<DB>>,
) -> tide::Result {
    let DeleteAccountData{ password } = req.body_form().await?;

    let session_id = get_session_id(&req)?;
    let user_name = {
        let session_store = req.state().session_store.lock().unwrap();
        match session_store.get(&session_id) {
            Some(Session::LoggedIn(UserInfo{ user_name, .. })) => user_name.clone(),
            _ => {
                return Err(tide::Error::from_str(
                    StatusCode::Forbidden,
                    format!("You need to log in in order to delete account.")
                ));
            }
        }
    };

    let account_id = req.state().secret_db.account_by_user_name(&user_name)
        .await?
        .and_then(Account::live)
        .ok_or(tide::Error::from_str(
            StatusCode::Forbidden,
            format!("User '{}' not found.", user_name),
        ))?
        .id;

    req.state().secret_db.delete_account_txn(account_id, Box::new(move |account| -> anyhow::Result<DeletedAccount> {
        if account.registration_method == RegistrationMethod::Password {
            authorize_access_by_password(&password.unwrap_or_default(), &account)?;
        }
        Ok(DeletedAccount {
            id: account.id,
            user_name: account.user_name,
            creation_time: account.creation_time,
            deletion_time: OffsetDateTime::now_utc(),
        })
    })).await.map_err(|err| tide::Error::new(StatusCode::Forbidden, err))?;

    req.state().session_store.lock().unwrap()
        .update_if_exists(&session_id, Session::logout);

    let mut resp: tide::Response = req.into();
    resp.set_status(StatusCode::Ok);
    Ok(resp)
}

pub async fn handle_mysession<DB>(req: tide::Request<HttpServerState<DB>>) -> tide::Result {
    let session_id = get_session_id(&req)?;
    let session_store = req.state().session_store.lock().unwrap();
    match session_store.get(&session_id) {
        Some(Session::Unknown) => {
            panic!("Session::Unknown is a client-only state. Should never happen on server.");
        }
        None | Some(Session::LoggedOut) => Ok("You are not logged in.".into()),
        Some(Session::GoogleOAuthRegistering(registration_info)) => Ok(format!(
            "You are currently signing up with Google in. \
                GoogleOAuthRegisteringInfo: {registration_info:?}"
        )
        .into()),
        Some(Session::LoggedIn(user_info)) => {
            Ok(format!("You are logged in. UserInfo: {user_info:?}").into())
        }
    }
}
