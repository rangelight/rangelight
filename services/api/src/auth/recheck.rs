//! Long-lived responses end when their login does (review M1 of RAN-37).

use connectrpc::{ConnectError, RequestContext};
use futures_util::{Stream, StreamExt};

use super::{Auth, IDLE, LoginHash};
use rl_store::StoreError;

impl Auth {
    /// Ends `stream` with UNAUTHENTICATED once the login of the request in `ctx` is gone
    /// (signed out, expired, idle, deleted), checked every the `recheck` of [`Auth::new`]; with UNAVAILABLE
    /// if Postgres can't say. Every long-lived response goes through here (the terminal WebSocket
    /// too, RAN-41).
    ///
    /// # Errors
    /// UNAUTHENTICATED if `ctx` carries no login (`require_login` didn't run).
    pub(crate) fn while_signed_in<T: Send + 'static>(
        &self,
        ctx: &RequestContext,
        stream: impl Stream<Item = Result<T, ConnectError>> + Send + 'static,
    ) -> Result<impl Stream<Item = Result<T, ConnectError>> + Send + 'static, ConnectError> {
        let login = *ctx
            .extensions()
            .get::<LoginHash>()
            .ok_or_else(|| ConnectError::unauthenticated("unauthenticated"))?;
        let every = self.0.recheck;
        let checks = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
        let state = (Box::pin(stream), checks, self.clone(), login);
        Ok(futures_util::stream::unfold(
            Some(state),
            |state| async move {
                let (mut stream, mut checks, auth, login) = state?;
                loop {
                    tokio::select! {
                        item = stream.next() => {
                            return item.map(|item| (item, Some((stream, checks, auth, login))));
                        }
                        _ = checks.tick() => {
                            if let Err(gone) = auth.still_signed_in(login).await {
                                return Some((Err(gone), None));
                            }
                        }
                    }
                }
            },
        ))
    }

    async fn still_signed_in(&self, login: LoginHash) -> Result<(), ConnectError> {
        let alive = self.0.store.login_alive(&login.0, IDLE).await;
        alive
            .map_err(|error| recheck_failed(&error))?
            .then_some(())
            .ok_or_else(login_gone)
    }
}

fn login_gone() -> ConnectError {
    tracing::info!("ended a stream whose login is gone");
    ConnectError::unauthenticated("unauthenticated")
}

fn recheck_failed(error: &StoreError) -> ConnectError {
    tracing::error!(%error, "login re-check failed");
    ConnectError::unavailable("store_unavailable")
}
