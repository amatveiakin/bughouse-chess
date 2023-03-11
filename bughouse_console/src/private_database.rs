use sqlx::prelude::*;
use tide::utils::async_trait;
use time::{OffsetDateTime, PrimitiveDateTime};

use crate::database::*;
use crate::private_persistence::*;

#[async_trait]
impl<DB> PrivateDatabaseReader for SqlxDatabase<DB>
where
    DB: sqlx::Database,
    for<'q> i64: sqlx::Type<DB> + sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB>,
    for<'q> String: sqlx::Type<DB> + sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB>,
    for<'q> bool: sqlx::Type<DB> + sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB>,
    for<'q> OffsetDateTime: sqlx::Type<DB> + sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB>,
    for<'q> PrimitiveDateTime: sqlx::Type<DB> + sqlx::Decode<'q, DB>,
    for<'c> &'c mut DB::Connection: sqlx::Executor<'c, Database = DB>,
    for<'a> <DB as sqlx::database::HasArguments<'a>>::Arguments: sqlx::IntoArguments<'a, DB>,
    for<'s> &'s str: sqlx::ColumnIndex<DB::Row>,
    usize: sqlx::ColumnIndex<DB::Row>,
{
    async fn account_by_email(&self, email: &str) -> Result<Account, anyhow::Error> {
        // TODO: handle NOT_FOUND separately from other errors.
        let row = self
            .pool
            .fetch_one(
                sqlx::query::<DB>(
                    "SELECT
                rowid,
                creation_time,
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
        Ok(Account {
            id: Some(AccountId(row.try_get("rowid")?)),
            creation_time: row.try_get("creation_time")?,
            user_name: row.try_get("user_name")?,
            email: row.try_get("email")?,
            password_hash: row.try_get("password_hash")?,
            registration_method: RegistrationMethod::try_from_string(
                row.try_get("registration_method")?,
            )
            .ok_or(anyhow::Error::msg("failed to parse registration method"))?,
        })
    }
    async fn account_by_user_name(&self, _user_name: &str) -> Result<Account, anyhow::Error> {
        todo!()
    }
}

#[async_trait]
impl<DB> PrivateDatabaseWriter for SqlxDatabase<DB>
where
    DB: sqlx::Database + HasRowidColumnDefinition,
    String: Type<DB> + for<'q> Encode<'q, DB>,
    Option<String>: Type<DB> + for<'q> Encode<'q, DB>,
    i64: Type<DB> + for<'q> Encode<'q, DB>,
    Option<i64>: Type<DB> + for<'q> Encode<'q, DB>,
    OffsetDateTime: Type<DB> + for<'q> Encode<'q, DB>,
    Option<OffsetDateTime>: Type<DB> + for<'q> Encode<'q, DB>,
    bool: Type<DB> + for<'q> Encode<'q, DB>,
    for<'c> &'c mut DB::Connection: sqlx::Executor<'c, Database = DB>,
    for<'a> <DB as sqlx::database::HasArguments<'a>>::Arguments: sqlx::IntoArguments<'a, DB>,
{
    async fn create_tables(&self) -> anyhow::Result<()> {
        let rowid_column_definition = DB::ROWID_COLUMN_DEFINITION;
        sqlx::query(
            format!(
                "CREATE TABLE IF NOT EXISTS accounts (
                {rowid_column_definition}
                creation_time TIMESTAMP,
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
    async fn create_account(&self, account: Account) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT into accounts(
                creation_time,
                user_name,
                email,
                password_hash,
                registration_method)
            VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(account.creation_time)
        .bind(account.user_name)
        .bind(account.email)
        .bind(account.password_hash)
        .bind(account.registration_method.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
    async fn update_account_txn(
        &self,
        _id: AccountId,
        _f: &(dyn FnOnce(&mut Account) + Send + Sync),
    ) -> anyhow::Result<()> {
        Err(anyhow::Error::msg(
            "create_account is unimplemented in UnimplementedDatabase",
        ))
    }
}

#[async_trait]
impl<DB> PrivateDatabaseRW for SqlxDatabase<DB>
where
    DB: sqlx::Database + HasRowidColumnDefinition,
    String: Type<DB> + for<'q> Encode<'q, DB>,
    Option<String>: Type<DB> + for<'q> Encode<'q, DB>,
    i64: Type<DB> + for<'q> Encode<'q, DB>,
    Option<i64>: Type<DB> + for<'q> Encode<'q, DB>,
    OffsetDateTime: Type<DB> + for<'q> Encode<'q, DB> + for<'q> sqlx::Decode<'q, DB>,
    Option<OffsetDateTime>: Type<DB> + for<'q> Encode<'q, DB>,
    bool: Type<DB> + for<'q> Encode<'q, DB>,
    for<'c> &'c mut DB::Connection: sqlx::Executor<'c, Database = DB>,
    for<'a> <DB as sqlx::database::HasArguments<'a>>::Arguments: sqlx::IntoArguments<'a, DB>,
    for<'q> i64: sqlx::Type<DB> + sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB>,
    for<'q> String: sqlx::Type<DB> + sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB>,
    for<'q> bool: sqlx::Type<DB> + sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB>,
    for<'q> OffsetDateTime: sqlx::Type<DB> + sqlx::Encode<'q, DB>,
    for<'q> PrimitiveDateTime: sqlx::Type<DB> + sqlx::Decode<'q, DB>,
    for<'c> &'c mut DB::Connection: sqlx::Executor<'c, Database = DB>,
    for<'a> <DB as sqlx::database::HasArguments<'a>>::Arguments: sqlx::IntoArguments<'a, DB>,
    for<'s> &'s str: sqlx::ColumnIndex<DB::Row>,
    usize: sqlx::ColumnIndex<DB::Row>,
{
}

#[async_trait]
impl PrivateDatabaseReader for UnimplementedDatabase {
    async fn account_by_email(&self, _email: &str) -> Result<Account, anyhow::Error> {
        Err(anyhow::Error::msg(
            "account_by_email is unimplemented in UnimplementedDatabase",
        ))
    }
    async fn account_by_user_name(&self, _user_name: &str) -> Result<Account, anyhow::Error> {
        Err(anyhow::Error::msg(
            "account_by_user_name is unimplemented in UnimplementedDatabase",
        ))
    }
}

#[async_trait]
impl PrivateDatabaseWriter for UnimplementedDatabase {
    async fn create_tables(&self) -> anyhow::Result<()> {
        Err(anyhow::Error::msg(
            "create_table is unimplemented in UnimplementedDatabase",
        ))
    }
    async fn create_account(&self, _account: Account) -> anyhow::Result<()> {
        Err(anyhow::Error::msg(
            "create_account is unimplemented in UnimplementedDatabase",
        ))
    }
    async fn update_account_txn(
        &self,
        _id: AccountId,
        _f: &(dyn FnOnce(&mut Account) + Send + Sync),
    ) -> anyhow::Result<()> {
        Err(anyhow::Error::msg(
            "create_account is unimplemented in UnimplementedDatabase",
        ))
    }
}

#[async_trait]
impl PrivateDatabaseRW for UnimplementedDatabase {}
