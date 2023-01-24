use anyhow::Context;
use oauth2::{basic::BasicClient, revocation::StandardRevocableToken, TokenResponse};
// Alternatively, this can be oauth2::curl::http_client or a custom.
use oauth2::reqwest::async_http_client;
use oauth2::{
    url, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge,
    RedirectUrl, RevocationUrl, Scope, TokenUrl,
};
use serde::Deserialize;
use std::env;
use url::Url;

use crate::session::UserInfo;

pub struct Config {
    // The URL where oauth proviver will redirect, passing the auth code
    // as a parameter.
    pub callback_url: String,
}

#[derive(Clone)]
pub struct GoogleAuth {
    client: BasicClient,
}

// Internal Google-specific user info struct, used for
// deserializing user info JSON responses from the API.
// https://cloud.google.com/identity-platform/docs/reference/rest/v1/UserInfo
#[derive(Deserialize)]
struct GoogleUserInfo {
    email: String,
    name: Option<String>,
}

// Internal OAuth-specific (Google-specific?) structure of the redirected
// URL parameters.
#[derive(Deserialize)]
pub struct NewSessionQuery {
    code: String,
    state: String,
}

impl NewSessionQuery {
    pub fn parse(self) -> (AuthorizationCode, CsrfToken) {
        (AuthorizationCode::new(self.code), CsrfToken::new(self.state))
    }
}

impl GoogleAuth {
    pub fn new(config: Config) -> anyhow::Result<Self> {
        // See https://accounts.google.com/.well-known/openid-configuration
        let google_client_id = ClientId::new(
            env::var("GOOGLE_CLIENT_ID")
                .context("Missing the GOOGLE_CLIENT_ID environment variable.")?,
        );
        let google_client_secret = ClientSecret::new(
            env::var("GOOGLE_CLIENT_SECRET")
                .context("Missing the GOOGLE_CLIENT_SECRET environment variable.")?,
        );
        let auth_url = AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_owned())
            .context("Invalid authorization endpoint URL")?;
        let token_url = TokenUrl::new("https://oauth2.googleapis.com/token".to_owned())
            .context("Invalid token endpoint URL")?;
        let client = BasicClient::new(
            google_client_id,
            Some(google_client_secret),
            auth_url,
            Some(token_url),
        )
        .set_revocation_uri(
            RevocationUrl::new("https://oauth2.googleapis.com/revoke".to_owned())
                .context("Invalid revocation endpoint URL")?,
        )
        .set_redirect_uri(RedirectUrl::new(config.callback_url).context("Invalid redirect URL")?);
        Ok(Self { client })
    }

    pub fn start(&self) -> anyhow::Result<(Url, CsrfToken)> {
        Ok(self
            .client
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new("openid profile email".to_owned()))
            .url())
    }

    pub async fn user_info(&self, code: AuthorizationCode) -> anyhow::Result<UserInfo> {
        let token_response = self
            .client
            .exchange_code(code)
            .request_async(async_http_client)
            .await?;
        let response = reqwest::get(format!(
            "https://www.googleapis.com/oauth2/v1/userinfo?access_token={}",
            token_response.access_token().secret()
        ))
        .await?
        .json::<GoogleUserInfo>()
        .await?;
        Ok(UserInfo {
            email: Some(response.email),
            name: response.name,
        })
    }
}
