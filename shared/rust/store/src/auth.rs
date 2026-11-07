use std::time::Duration;

use rl_domain::Member;

use crate::{Store, StoreError};

/// A login to create for a verified identity.
#[derive(Debug)]
pub struct NewLogin<'a> {
    /// SHA-256 of the cookie's token.
    pub token_hash: [u8; 32],
    /// Who signed in.
    pub member: &'a Member,
    /// The ID token's `iss`.
    pub issuer: &'a str,
    /// The ID token's `sub`.
    pub subject: &'a str,
    /// How long the login lasts, however active.
    pub absolute: Duration,
    /// Logins unused this long are deleted as this one is created.
    pub idle: Duration,
}

impl Store {
    /// Creates a login, and deletes the logins past their end or idle. The member is bound to
    /// `issuer` and `subject` on its first sign-in; `false` (and no login) when another account
    /// already owns it.
    ///
    /// # Errors
    /// [`StoreError::Db`].
    pub async fn sign_in(&self, login: &NewLogin<'_>) -> Result<bool, StoreError> {
        let row = sqlx::query_file!(
            "../../../db/queries/auth/sign_in.sql",
            &login.token_hash[..],
            login.member.as_str(),
            login.issuer,
            login.subject,
            login.absolute.as_secs_f64(),
            login.idle.as_secs_f64(),
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.is_some())
    }

    /// The member of a live login (not past its end, not idle for `idle`), marking it used now.
    ///
    /// # Errors
    /// [`StoreError::Db`].
    pub async fn login_member(
        &self,
        token_hash: &[u8; 32],
        idle: Duration,
    ) -> Result<Option<Member>, StoreError> {
        let row = sqlx::query_file!(
            "../../../db/queries/auth/touch.sql",
            &token_hash[..],
            idle.as_secs_f64(),
        )
        .fetch_optional(&self.pool)
        .await?;
        // The column's CHECK holds the same rule, so a stored member always parses.
        row.map(|row| Member::from_username(&row.member).map_err(StoreError::from))
            .transpose()
    }

    /// Whether a login lives (not past its end, not idle for `idle`), without counting as use.
    ///
    /// # Errors
    /// [`StoreError::Db`].
    pub async fn login_alive(
        &self,
        token_hash: &[u8; 32],
        idle: Duration,
    ) -> Result<bool, StoreError> {
        let row = sqlx::query_file!(
            "../../../db/queries/auth/alive.sql",
            &token_hash[..],
            idle.as_secs_f64(),
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.alive)
    }

    /// Deletes a login. Deleting one that doesn't exist is not an error.
    ///
    /// # Errors
    /// [`StoreError::Db`].
    pub async fn sign_out(&self, token_hash: &[u8; 32]) -> Result<(), StoreError> {
        sqlx::query_file!("../../../db/queries/auth/sign_out.sql", &token_hash[..])
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
