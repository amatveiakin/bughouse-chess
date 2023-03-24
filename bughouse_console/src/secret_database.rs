use sqlx::prelude::*;
use tide::utils::async_trait;
use time::OffsetDateTime;

use bughouse_chess::session::RegistrationMethod;

use crate::database::*;
use crate::secret_persistence::*;

#[async_trait]
impl<DB> SecretDatabaseReader for SqlxDatabase<DB>
where
    DB: sqlx::Database,
    for<'q> i64: sqlx::Type<DB> + sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB>,
    for<'q> String: sqlx::Type<DB> + sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB>,
    for<'q> bool: sqlx::Type<DB> + sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB>,
    for<'q> OffsetDateTime: sqlx::Type<DB> + sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB>,
    for<'c> &'c mut DB::Connection: sqlx::Executor<'c, Database = DB>,
    for<'a> <DB as sqlx::database::HasArguments<'a>>::Arguments: sqlx::IntoArguments<'a, DB>,
    for<'s> &'s str: sqlx::ColumnIndex<DB::Row>,
    usize: sqlx::ColumnIndex<DB::Row>,
{
    async fn account_by_email(&self, email: &str) -> Result<Option<Account>, anyhow::Error> {
        let row = self
            .pool
            .fetch_optional(
                sqlx::query::<DB>(
                    "SELECT
                        rowid,
                        deleted,
                        creation_time,
                        deletion_time,
                        user_name,
                        email,
                        password_hash,
                        registration_method
                     FROM accounts
                     WHERE
                        email=$1",
                )
                .bind(email.to_owned()),
            )
            .await?;
        row.map(row_to_account).transpose()
    }

    async fn account_by_user_name(&self, user_name: &str) -> Result<Option<Account>, anyhow::Error> {
        // TODO: handle NOT_FOUND separately from other errors.
        let row = self
            .pool
            .fetch_optional(
                sqlx::query::<DB>(
                    "SELECT
                        rowid,
                        deleted,
                        creation_time,
                        deletion_time,
                        user_name,
                        email,
                        password_hash,
                        registration_method
                     FROM accounts
                     WHERE
                        user_name=$1",
                )
                .bind(user_name.to_owned()),
            )
            .await?;
        row.map(row_to_account).transpose()
    }
}

fn row_to_account<DB>(row: DB::Row) -> Result<Account, anyhow::Error>
where
    // TODO: Deduplicate trait requirements.
    DB: sqlx::Database,
    for<'q> i64: sqlx::Type<DB> + sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB>,
    for<'q> String: sqlx::Type<DB> + sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB>,
    for<'q> bool: sqlx::Type<DB> + sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB>,
    for<'q> OffsetDateTime: sqlx::Type<DB> + sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB>,
    for<'c> &'c mut DB::Connection: sqlx::Executor<'c, Database = DB>,
    for<'a> <DB as sqlx::database::HasArguments<'a>>::Arguments: sqlx::IntoArguments<'a, DB>,
    for<'s> &'s str: sqlx::ColumnIndex<DB::Row>,
    usize: sqlx::ColumnIndex<DB::Row>,
{
    let id = AccountId(row.try_get("rowid")?);
    let creation_time = row.try_get("creation_time")?;
    let user_name = row.try_get("user_name")?;
    let deleted: bool = row.try_get("deleted")?;
    if deleted {
        return Ok(Account::Deleted(DeletedAccount {
            id,
            user_name,
            creation_time,
            deletion_time: row.try_get("deletion_time")?,
        }));
    }
    Ok(Account::Live(LiveAccount {
        id,
        creation_time,
        user_name,
        email: row.try_get("email")?,
        password_hash: row.try_get("password_hash")?,
        registration_method: RegistrationMethod::try_from_string(
            row.try_get("registration_method")?,
        ).map_err(anyhow::Error::msg)?,
    }))
}

#[async_trait]
impl<DB> SecretDatabaseWriter for SqlxDatabase<DB>
where
    DB: sqlx::Database + HasRowidColumnDefinition,
    for<'q> String: Type<DB> + Encode<'q, DB> + Decode<'q, DB>,
    for<'q> Option<String>: Type<DB> + Encode<'q, DB> + Decode<'q, DB>,
    for<'q> i64: Type<DB> + Encode<'q, DB> + Decode<'q, DB>,
    Option<i64>: Type<DB> + for<'q> Encode<'q, DB>,
    for<'q> OffsetDateTime: Type<DB> + Encode<'q, DB> + Decode<'q, DB>,
    Option<OffsetDateTime>: Type<DB> + for<'q> Encode<'q, DB>,
    for<'q> bool: sqlx::Type<DB> + sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB>,
    for<'c> &'c mut DB::Connection: sqlx::Executor<'c, Database = DB>,
    for<'a> <DB as sqlx::database::HasArguments<'a>>::Arguments: sqlx::IntoArguments<'a, DB>,
    for<'s> &'s str: sqlx::ColumnIndex<DB::Row>,
    usize: sqlx::ColumnIndex<DB::Row>,
{
    async fn create_tables(&self) -> anyhow::Result<()> {
        let rowid_column_definition = DB::ROWID_COLUMN_DEFINITION;
        sqlx::query(
            format!(
                "CREATE TABLE IF NOT EXISTS accounts (
                {rowid_column_definition}
                deleted BOOLEAN DEFAULT FALSE,
                creation_time TIMESTAMP,
                deletion_time TIMESTAMP,
                user_name TEXT UNIQUE,
                email TEXT UNIQUE,
                password_hash TEXT,
                registration_method TEXT
                )",
            )
            .as_str(),
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn create_account(
        &self,
        user_name: String,
        email: Option<String>,
        password_hash: Option<String>,
        registration_method: RegistrationMethod,
        creation_time: OffsetDateTime,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO accounts(
                creation_time,
                user_name,
                email,
                password_hash,
                registration_method)
            VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(creation_time)
        .bind(user_name)
        .bind(email)
        .bind(password_hash)
        .bind(registration_method.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // Updates account atomically. Fails if the account does not exist.
    async fn update_account_txn(
        &self,
        id: AccountId,
        f: Box<dyn for<'a> FnOnce(&'a mut LiveAccount) -> anyhow::Result<()> + Send>,
    ) -> anyhow::Result<()> {
        let mut txn = self.pool.begin().await?;
        let row = txn
            .fetch_one(
                sqlx::query::<DB>(
                    "SELECT
                        rowid,
                        deleted,
                        creation_time,
                        deletion_time,
                        user_name,
                        email,
                        password_hash,
                        registration_method
                     FROM accounts
                     WHERE
                        rowid=$1",
                )
                .bind(id.0),
            )
            .await?;
        let account = row_to_account(row)?;
        let Account::Live(mut live_account) = account else {
            return Err(anyhow::anyhow!("Cannot update deleted account."));
        };
        f(&mut live_account)?;
        txn.execute(
            sqlx::query(
                "UPDATE accounts SET
                    creation_time=$1,
                    user_name=$2,
                    email=$3,
                    password_hash=$4,
                    registration_method=$5
                WHERE rowid=$6",
            )
            .bind(live_account.creation_time)
            .bind(live_account.user_name)
            .bind(live_account.email)
            .bind(live_account.password_hash)
            .bind(live_account.registration_method.to_string())
            .bind(id.0),
        )
        .await?;
        txn.commit().await?;
        Ok(())
    }

    async fn delete_account_txn(
        &self,
        id: AccountId,
        f: Box<dyn FnOnce(LiveAccount) -> anyhow::Result<DeletedAccount> + Send>,
    ) -> anyhow::Result<()> {

        let mut txn = self.pool.begin().await?;
        let row = txn
            .fetch_one(
                sqlx::query::<DB>(
                    "SELECT
                        rowid,
                        deleted,
                        creation_time,
                        deletion_time,
                        user_name,
                        email,
                        password_hash,
                        registration_method
                     FROM accounts
                     WHERE
                        rowid=$1",
                )
                .bind(id.0),
            )
            .await?;
        let account = row_to_account(row)?;
        let Account::Live(live_account) = account else {
            return Err(anyhow::anyhow!("Cannot delete deleted account."));
        };
        let deleted_account = f(live_account)?;
        txn.execute(
            sqlx::query(
                "UPDATE accounts SET
                    deleted=TRUE,
                    creation_time=$1,
                    deletion_time=$2,
                    user_name=$3,
                    email=NULL,
                    password_hash=NULL,
                    registration_method=NULL
                WHERE rowid=$4",
            )
            .bind(deleted_account.creation_time)
            .bind(deleted_account.deletion_time)
            .bind(deleted_account.user_name)
            .bind(id.0),
        )
        .await?;
        txn.commit().await?;
        Ok(())
    }
}

#[async_trait]
impl<D: SecretDatabaseReader + SecretDatabaseWriter + Send + Sync> SecretDatabaseRW for D {}

#[async_trait]
impl SecretDatabaseReader for UnimplementedDatabase {
    async fn account_by_email(&self, _email: &str) -> Result<Option<Account>, anyhow::Error> {
        Err(anyhow::Error::msg(
            "account_by_email is unimplemented in UnimplementedDatabase",
        ))
    }
    async fn account_by_user_name(&self, _user_name: &str) -> Result<Option<Account>, anyhow::Error> {
        Err(anyhow::Error::msg(
            "account_by_user_name is unimplemented in UnimplementedDatabase",
        ))
    }
}

#[async_trait]
impl SecretDatabaseWriter for UnimplementedDatabase {
    async fn create_tables(&self) -> anyhow::Result<()> {
        Err(anyhow::Error::msg(
            "create_table is unimplemented in UnimplementedDatabase",
        ))
    }
    async fn create_account(
        &self,
        _user_name: String,
        _email: Option<String>,
        _password_hash: Option<String>,
        _registration_method: RegistrationMethod,
        _creation_time: OffsetDateTime,
    ) -> anyhow::Result<()> {
        Err(anyhow::Error::msg(
            "create_account is unimplemented in UnimplementedDatabase",
        ))
    }
    async fn update_account_txn(
        &self,
        _id: AccountId,
        _f: Box<dyn for<'a> FnOnce(&'a mut LiveAccount) -> anyhow::Result<()> + Send>,
    ) -> anyhow::Result<()> {
        Err(anyhow::Error::msg(
            "create_account is unimplemented in UnimplementedDatabase",
        ))
    }
    async fn delete_account_txn(
        &self,
        _id: AccountId,
        _f: Box<dyn FnOnce(LiveAccount) -> anyhow::Result<DeletedAccount> + Send>,
    ) -> anyhow::Result<()> {
        Err(anyhow::Error::msg(
            "delete_account is unimplemented in UnimplementedDatabase",
        ))
    }
}
