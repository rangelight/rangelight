//! Long-lived responses end when their login does (review M1 of RAN-37).

use connectrpc::{ConnectError, RequestContext};
use futures_util::{Stream, StreamExt};

use super::{Auth, IDLE, LoginHash};
use crate::access::{Access, logged};
use rl_domain::Member;
use rl_store::StoreError;

impl Auth {
    /// Ends `stream` with UNAUTHENTICATED once the login of the request in `ctx` is gone, or with
    /// UNAVAILABLE if Postgres can't say ([`Auth::ended`]), and logs when it opens and ends. Every
    /// long-lived Connect response goes through here; the terminal WebSocket waits on
    /// [`Auth::ended`] itself.
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
        // One line when it opens and one when it ends, with why (crate::access).
        let member = ctx
            .extensions()
            .get::<Member>()
            .map(|m| m.as_str().to_owned());
        let access = ctx.extensions().get::<Access>().cloned();
        let name = ctx.path().unwrap_or_default().to_owned();
        let state = (Box::pin(stream), Box::pin(self.clone().ended(login)));
        let stream = futures_util::stream::unfold(Some(state), |state| async move {
            let (mut stream, mut ended) = state?;
            tokio::select! {
                item = stream.next() => item.map(|item| (item, Some((stream, ended)))),
                gone = &mut ended => Some((Err(gone), None)),
            }
        });
        // Around the whole stream, so its end line says what the client got (a gone login too).
        Ok(logged(stream, name, member.unwrap_or_default(), access))
    }

    /// Resolves once `login` is gone (signed out, expired, idle, deleted), checked every `recheck`
    /// of [`Auth::new`]: with UNAUTHENTICATED, or UNAVAILABLE if Postgres can't say.
    pub(crate) async fn ended(self, login: LoginHash) -> ConnectError {
        let every = self.0.recheck;
        let mut checks = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
        loop {
            checks.tick().await;
            if let Err(gone) = self.still_signed_in(login).await {
                return gone;
            }
        }
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
