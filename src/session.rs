use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegistrationMethod {
    Password,
    GoogleOAuth,
    LichessOAuth,
}

impl RegistrationMethod {
    // Don't use `Display` because this is for stable serialization, not for human consumption.
    #[allow(clippy::inherent_to_string)]
    pub fn to_string(self) -> String {
        match self {
            Self::Password => "Password",
            Self::GoogleOAuth => "GoogleOAuth",
            Self::LichessOAuth => "LichessOAuth",
        }
        .to_owned()
    }
    pub fn try_from_string(s: String) -> Result<Self, String> {
        match s.as_str() {
            "Password" => Ok(Self::Password),
            "GoogleOAuth" => Ok(Self::GoogleOAuth),
            "LichessOAuth" => Ok(Self::LichessOAuth),
            _ => Err(format!("failed to parse '{s}' as RegistrationMethod")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub user_name: String,
    pub email: Option<String>,
    pub lichess_user_id: Option<String>,
    pub registration_method: RegistrationMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleOAuthRegistrationInfo {
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LichessOAuthRegistrationInfo {
    pub user_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PkceChallengeInfo {
    pub verifier: String,
}


#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum Session {
    Unknown,
    #[default]
    LoggedOut,
    PkceChallengeInitiated(PkceChallengeInfo),
    GoogleOAuthRegistering(GoogleOAuthRegistrationInfo), // in the midst of Google OAuth signup
    LichessOAuthRegistering(LichessOAuthRegistrationInfo), // in the midst of Google OAuth signup
    LoggedIn(UserInfo),
}


impl Session {
    pub fn user_info(&self) -> Option<&UserInfo> {
        match self {
            Session::Unknown
            | Session::LoggedOut
            | Session::PkceChallengeInitiated(_)
            | Session::GoogleOAuthRegistering(_)
            | Session::LichessOAuthRegistering(_) => None,
            Session::LoggedIn(user_info) => Some(user_info),
        }
    }
    pub fn user_name(&self) -> Option<&str> { self.user_info().map(|info| info.user_name.as_str()) }
    pub fn logout(&mut self) { *self = Session::LoggedOut; }
}
