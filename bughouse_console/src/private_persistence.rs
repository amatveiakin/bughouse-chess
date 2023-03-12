use tide::utils::async_trait;
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy)]
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
    pub fn try_from_string(s: String) -> Option<Self> {
        match s.as_str() {
            "Password" => Some(Self::Password),
            "GoogleOAuth" => Some(Self::GoogleOAuth),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AccountId(pub i64);

#[derive(Debug, Clone)]
pub struct Account {
    pub id: Option<AccountId>,
    pub user_name: Option<String>,
    pub email: Option<String>,
    pub password_hash: Option<String>,
    pub registration_method: RegistrationMethod,
    pub creation_time: OffsetDateTime,
}

#[async_trait]
pub trait PrivateDatabaseReader {
    async fn account_by_email(&self, email: &str) -> Result<Account, anyhow::Error>;
    async fn account_by_user_name(&self, user_name: &str) -> Result<Account, anyhow::Error>;
}

#[async_trait]
pub trait PrivateDatabaseWriter {
    async fn create_tables(&self) -> anyhow::Result<()>;
    async fn create_account(&self, account: Account) -> anyhow::Result<()>;
    async fn update_account_txn(
        &self,
        id: AccountId,
        f: Box<dyn for<'a>FnOnce(&'a mut Account) + Send>,
    ) -> anyhow::Result<()>;
}

#[async_trait]
pub trait PrivateDatabaseRW: PrivateDatabaseWriter + PrivateDatabaseReader + Send + Sync {}
