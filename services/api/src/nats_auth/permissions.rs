//! What each identity may do on NATS, as its user JWT says (ADR 0021 amendment, ADR 0043): the
//! subject lists of `rl_domain::subject`, and which account it joins. Limits are the accounts'
//! (`files/nats/auth.conf`): the server doesn't apply a callout user's own.
//!
//! ponytail: no publish-rate limit, which NATS doesn't have per user. Ceiling: a lane can flood
//! its own output subject. Upgrade: the api rate-limits what it relays from a lane (ADR 0043).

use rl_domain::subject::{self, Permissions};

use super::identity::Identity;

/// The account an identity joins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Account {
    /// The control plane's and the runners' (`nats_auth.account`).
    Main,
    /// The lanes': one subscription, 128 KiB, no JetStream (`nats_auth.lane_account`).
    Lane,
}

/// One identity's user JWT content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Grant {
    /// The user's name, which is also its inbox id (`subject::inbox`).
    pub(crate) name: String,
    /// What it may publish and subscribe to.
    pub(crate) permissions: Permissions,
    /// Where it joins.
    pub(crate) account: Account,
}

/// The grant of `identity` in `org`.
pub(crate) fn grant(org: &str, identity: &Identity) -> Grant {
    let main = |name: String, permissions| Grant {
        name,
        permissions,
        account: Account::Main,
    };
    match identity {
        Identity::Api => main("api".into(), subject::api(org)),
        Identity::Relay => main("relay".into(), subject::relay(org)),
        Identity::Runner(runner) => main(
            subject::desired_consumer(runner),
            subject::runner(org, runner),
        ),
        // One connection per session: the callout refuses a second.
        Identity::Lane(session) => Grant {
            name: format!("lane-{}", session.0),
            permissions: subject::lane(org, *session),
            account: Account::Lane,
        },
    }
}
