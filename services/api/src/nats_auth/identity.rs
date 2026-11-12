//! Who a verified ServiceAccount token is (ADR 0021 amendment). Everything not listed here is
//! refused: the api and the relay by their ServiceAccounts in the control plane's namespace, a
//! runner by its ServiceAccount in its own cluster, and a lane by its namespace and its pod.

use rl_domain::SessionId;

use crate::config::Issuer;

/// The namespace the chart installs the control plane into (it refuses any other).
pub(crate) const CONTROL_PLANE_NAMESPACE: &str = "rangelight-system";
/// The api's ServiceAccount there.
pub(crate) const API_ACCOUNT: &str = "rangelight-api";
/// The relay's ServiceAccount there.
pub(crate) const RELAY_ACCOUNT: &str = "rangelight-relay";
/// The ServiceAccount a lane's agent pod runs as, in its namespace (the runner's `agentPod`).
pub(crate) const LANE_ACCOUNT: &str = "default";
/// The lane's agent pod, the only pod whose token is a lane (the runner's `agentName`).
pub(crate) const LANE_POD: &str = "agent";
/// The prefix of every lane namespace (the runner's `Namespace`).
pub(crate) const LANE_NAMESPACE_PREFIX: &str = "rl-";

/// A NATS client's identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    /// The api.
    Api,
    /// The relay.
    Relay,
    /// The runner with this id.
    Runner(String),
    /// The lane of this live session.
    Lane(SessionId),
}

/// Who a token says it is, before a lane's namespace is looked up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Claim {
    /// Known from the token alone.
    Known(Identity),
    /// A lane, if a live session's namespace is this one.
    Lane(String),
}

/// Who the ServiceAccount `namespace:account`, in `pod`, from `issuer` is. A lane is only the
/// `agent` pod of an `rl-` namespace, from the cluster of `sessions_runner`, the runner every
/// session runs at: anything else never reaches the Postgres lookup. `None`: no one.
pub(crate) fn claim(
    issuer: &Issuer,
    sessions_runner: &str,
    namespace: &str,
    account: &str,
    pod: Option<&str>,
) -> Option<Claim> {
    if issuer.control_plane && namespace == CONTROL_PLANE_NAMESPACE {
        match account {
            API_ACCOUNT => return Some(Claim::Known(Identity::Api)),
            RELAY_ACCOUNT => return Some(Claim::Known(Identity::Relay)),
            _ => {}
        }
    }
    if issuer.runner_service_account.split_once(':') == Some((namespace, account)) {
        return Some(Claim::Known(Identity::Runner(issuer.runner.clone())));
    }
    if issuer.runner == sessions_runner
        && account == LANE_ACCOUNT
        && pod == Some(LANE_POD)
        && namespace.starts_with(LANE_NAMESPACE_PREFIX)
    {
        return Some(Claim::Lane(namespace.to_owned()));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issuer(control_plane: bool, runner: &str) -> Issuer {
        Issuer {
            url: "https://kubernetes.default.svc.cluster.local".into(),
            jwks_url: "https://kubernetes.default.svc/openid/v1/jwks".into(),
            jwks_token_file: None,
            jwks_ca_file: None,
            control_plane,
            runner: runner.into(),
            runner_service_account: "rangelight-runner:runner".into(),
        }
    }

    #[test]
    fn each_identity_by_its_service_account() {
        let home = issuer(true, "r1");
        let api = claim(&home, "r1", "rangelight-system", "rangelight-api", None);
        assert_eq!(api, Some(Claim::Known(Identity::Api)));
        let relay = claim(&home, "r1", "rangelight-system", "rangelight-relay", None);
        assert_eq!(relay, Some(Claim::Known(Identity::Relay)));
        let runner = claim(&home, "r1", "rangelight-runner", "runner", None);
        assert_eq!(runner, Some(Claim::Known(Identity::Runner("r1".into()))));
        let lane = claim(&home, "r1", "rl-ran-7", "default", Some("agent"));
        assert_eq!(lane, Some(Claim::Lane("rl-ran-7".into())));
    }

    #[test]
    fn everything_else_is_no_one() {
        let home = issuer(true, "r1");
        let remote = issuer(false, "r2");
        for (issuer, namespace, account) in [
            // Another workload in the control plane's namespace.
            (&home, "rangelight-system", "rangelight-web"),
            // The control plane's names, in a runner's own cluster.
            (&remote, "rangelight-system", "rangelight-api"),
            (&remote, "rangelight-system", "rangelight-relay"),
            // Another account in the runner's namespace, or the runner's name elsewhere.
            (&home, "rangelight-runner", "default"),
            (&home, "other", "runner"),
            // A lane namespace's other accounts.
            (&home, "rl-ran-7", "agent"),
        ] {
            // At most a lane claim, which no live session's namespace answers.
            let who = claim(issuer, "r1", namespace, account, Some("agent"));
            assert!(
                !matches!(who, Some(Claim::Known(_))),
                "{namespace}:{account} is {who:?}"
            );
        }
        // Lanes only from the cluster whose runner the sessions run at.
        assert_eq!(
            claim(&remote, "r1", "rl-ran-7", "default", Some("agent")),
            None
        );
        // A remote cluster's runner is that runner, never the home one.
        let r2 = claim(&remote, "r1", "rangelight-runner", "runner", None);
        assert_eq!(r2, Some(Claim::Known(Identity::Runner("r2".into()))));
    }

    /// RAN-43 review 3a, 7: only the agent pod of an `rl-` namespace is a lane, decided before any
    /// lookup, so another pod in a lane's namespace, a token bound to no pod, or any other
    /// namespace's default account never costs a Postgres query.
    #[test]
    fn a_lane_is_only_the_agent_pod_of_an_rl_namespace() {
        let home = issuer(true, "r1");
        for (namespace, pod) in [
            ("rl-ran-7", None),
            ("rl-ran-7", Some("intruder")),
            ("team-a", Some("agent")),
            ("kube-system", Some("agent")),
        ] {
            assert_eq!(
                claim(&home, "r1", namespace, "default", pod),
                None,
                "{namespace} {pod:?}"
            );
        }
    }
}
