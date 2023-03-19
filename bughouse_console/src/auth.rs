use anyhow::Context;
use oauth2::{basic::BasicClient, TokenResponse};
// Alternatively, this can be oauth2::curl::http_client or a custom.
use oauth2::reqwest::async_http_client;
use oauth2::{
    url, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, RedirectUrl, RevocationUrl,
    Scope, TokenUrl,
};
use serde::Deserialize;
use std::env;
use url::Url;


#[derive(Clone)]
pub struct GoogleAuth {
    client: BasicClient,
}

// Internal Google-specific user info struct, used for
// deserializing user info JSON responses from the API.
// https://any-api.com/googleapis_com/oauth2/docs/userinfo/oauth2_userinfo_get
#[derive(Deserialize)]
struct GoogleUserInfo {
    email: String,
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
        (
            AuthorizationCode::new(self.code),
            CsrfToken::new(self.state),
        )
    }
}

impl GoogleAuth {
    pub fn new() -> anyhow::Result<Self> {
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
        );
        Ok(Self { client })
    }

    pub fn start(&self, callback_url: String) -> anyhow::Result<(Url, CsrfToken)> {
        Ok(self
            .client
            .clone()
            .set_redirect_uri(RedirectUrl::new(callback_url)?)
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new("email".to_owned()))
            .url())
    }

    pub async fn email(&self, callback_url: String, code: AuthorizationCode) -> anyhow::Result<String> {
        let token_response = self
            .client
            .clone()
            .set_redirect_uri(RedirectUrl::new(callback_url)?)
            .exchange_code(code)
            .request_async(async_http_client)
            .await.context("exchanging auth code for auth token failed")?;
        let response = reqwest::get(format!(
            "https://www.googleapis.com/oauth2/v1/userinfo?access_token={}",
            token_response.access_token().secret()
        ))
        .await.context("requesting user info failed")?
        .json::<GoogleUserInfo>()
        .await.context("getting user info JSON failed")?;
        Ok(response.email)
    }
}
