use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegistrationMethod {
    Password,
    GoogleOAuth,
}

impl RegistrationMethod {
    pub fn to_string(self) -> String {
        match self {
            Self::Password => "Password",
            Self::GoogleOAuth => "GoogleOAuth",
        }.to_owned()
    }
    pub fn try_from_string(s: String) -> Result<Self, String> {
        match s.as_str() {
            "Password" => Ok(Self::Password),
            "GoogleOAuth" => Ok(Self::GoogleOAuth),
            _ => Err(format!("failed to parse '{s}' as RegistrationMethod")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub user_name: String,
    pub email: Option<String>,
    pub registration_method: RegistrationMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleOAuthRegistrationInfo {
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Session {
    LoggedOut,
    GoogleOAuthRegistering(GoogleOAuthRegistrationInfo),
    LoggedIn(UserInfo),
}

impl Default for Session {
    fn default() -> Self {
        Session::LoggedOut
    }
}

impl Session {
    pub fn user_info(&self) -> Option<&UserInfo> {
        match self {
            Session::LoggedOut | Session::GoogleOAuthRegistering(_) => None,
            Session::LoggedIn(session) => Some(&session),
        }
    }
    pub fn logout(&mut self) {
        *self = Session::LoggedOut;
    }
}
