use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserInfo {
    pub email: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub logged_in: bool,
    pub user_info: UserInfo,
}

impl Default for Session {
    fn default() -> Self {
        Session {
            logged_in: false,
            user_info: UserInfo::default(),
        }
    }
}
