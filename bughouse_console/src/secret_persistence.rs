use tide::utils::async_trait;
use time::OffsetDateTime;

use bughouse_chess::session::RegistrationMethod;


#[derive(Debug, Clone, Copy)]
pub struct AccountId(pub i64);

#[derive(Debug, Clone)]
pub struct Account {
    pub id: AccountId,
    pub user_name: String,
    pub email: Option<String>,
    pub password_hash: Option<String>,
    pub registration_method: RegistrationMethod,
    pub creation_time: OffsetDateTime,
}

#[async_trait]
pub trait SecretDatabaseReader {
    async fn account_by_email(&self, email: &str) -> Result<Option<Account>, anyhow::Error>;
    async fn account_by_user_name(&self, user_name: &str) -> Result<Option<Account>, anyhow::Error>;
}

#[async_trait]
pub trait SecretDatabaseWriter {
    async fn create_tables(&self) -> anyhow::Result<()>;
    async fn create_account(
        &self,
        user_name: String,
        email: Option<String>,
        password_hash: Option<String>,
        registration_method: RegistrationMethod,
        creation_time: OffsetDateTime,
    ) -> anyhow::Result<()>;
    async fn update_account_txn(
        &self,
        id: AccountId,
        f: Box<dyn for<'a>FnOnce(&'a mut Account) + Send>,
    ) -> anyhow::Result<()>;
}

#[async_trait]
pub trait SecretDatabaseRW: SecretDatabaseWriter + SecretDatabaseReader + Send + Sync {}
