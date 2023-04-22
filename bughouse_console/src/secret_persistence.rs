use bughouse_chess::session::{RegistrationMethod, Session};
use bughouse_chess::session_store::SessionId;
use tide::utils::async_trait;
use time::OffsetDateTime;


#[derive(Debug, Clone, Copy)]
pub struct AccountId(pub i64);

#[derive(Debug, Clone)]
pub struct LiveAccount {
    pub id: AccountId,
    pub user_name: String,
    pub email: Option<String>,
    pub password_hash: Option<String>,
    pub registration_method: RegistrationMethod,
    pub creation_time: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct DeletedAccount {
    pub id: AccountId,
    pub user_name: String,
    pub creation_time: OffsetDateTime,
    pub deletion_time: OffsetDateTime,
}

pub enum Account {
    Live(LiveAccount),
    Deleted(DeletedAccount),
}

impl Account {
    pub fn live(self) -> Option<LiveAccount> {
        match self {
            Account::Live(live) => Some(live),
            Account::Deleted(_) => None,
        }
    }
}


#[async_trait]
pub trait SecretDatabaseReader {
    async fn account_by_email(&self, email: &str) -> Result<Option<Account>, anyhow::Error>;
    async fn account_by_user_name(&self, user_name: &str)
        -> Result<Option<Account>, anyhow::Error>;
    async fn list_sessions(&self) -> Result<Vec<(SessionId, Session)>, anyhow::Error>;
}

#[async_trait]
pub trait SecretDatabaseWriter {
    async fn create_tables(&self) -> anyhow::Result<()>;
    async fn create_account(
        &self, user_name: String, email: Option<String>, password_hash: Option<String>,
        registration_method: RegistrationMethod, creation_time: OffsetDateTime,
    ) -> anyhow::Result<()>;
    async fn update_account_txn(
        &self, id: AccountId,
        f: Box<dyn for<'a> FnOnce(&'a mut LiveAccount) -> anyhow::Result<()> + Send>,
    ) -> anyhow::Result<()>;
    async fn delete_account_txn(
        &self, id: AccountId,
        f: Box<dyn FnOnce(LiveAccount) -> anyhow::Result<DeletedAccount> + Send>,
    ) -> anyhow::Result<()>;
    async fn set_logged_in_session(
        &self, id: &SessionId, user_name: Option<String>, expiration: OffsetDateTime,
    ) -> anyhow::Result<()>;
    async fn gc_expired_sessions(&self) -> anyhow::Result<()>;
}

#[async_trait]
pub trait SecretDatabaseRW: SecretDatabaseWriter + SecretDatabaseReader + Send + Sync {}
