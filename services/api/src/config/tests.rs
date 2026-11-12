#![allow(clippy::unwrap_used)]

use super::*;

const REQUIRED: &str = "
linear_team = \"RAN\"
public_url = \"https://rangelight.example.com\"
oidc_issuer = \"https://rangelight.example.com/dex\"
repo = \"shop\"
member = \"ariel\"
";

fn defaults() -> Config {
    // Before the defaults' tables, so they stay top-level keys.
    let toml = [
        REQUIRED,
        include_str!("../../../../configs/api.default.toml"),
    ]
    .concat();
    rl_config::load("api", &toml).unwrap()
}

#[test]
fn defaults_with_the_required_keys_are_valid() {
    defaults().validate().unwrap();
}

#[test]
fn the_team_is_required_and_the_org_checked() {
    let defaults_only = include_str!("../../../../configs/api.default.toml");
    assert!(
        rl_config::load::<Config>("api", defaults_only).is_err(),
        "no default team"
    );
    for team in ["", "RAN.*", "a-very-long-team-key"] {
        assert!(
            Config {
                linear_team: team.into(),
                ..defaults()
            }
            .validate()
            .is_err(),
            "{team:?}"
        );
    }
    assert!(
        Config {
            org: "a b".into(),
            ..defaults()
        }
        .validate()
        .is_err()
    );
}

#[test]
fn sign_in_needs_https_and_an_origin() {
    let url = |s: &str| Url::parse(s).unwrap();
    for public_url in [
        "http://rangelight.example.com",
        "https://rangelight.example.com/workbench",
        "https://rangelight.example.com/?a=b",
        "https://user@rangelight.example.com",
    ] {
        let config = Config {
            public_url: url(public_url),
            ..defaults()
        };
        assert!(config.validate().is_err(), "{public_url}");
    }
    let plain_issuer = Config {
        oidc_issuer: url("http://dex.example.com/dex"),
        ..defaults()
    };
    assert!(plain_issuer.validate().is_err());
    let no_client = Config {
        oidc_client_id: String::new(),
        ..defaults()
    };
    assert!(no_client.validate().is_err());
    let no_claim = Config {
        oidc_member_claim: String::new(),
        ..defaults()
    };
    assert!(no_claim.validate().is_err());
    let no_recheck = Config {
        login_recheck_seconds: 0,
        ..defaults()
    };
    assert!(no_recheck.validate().is_err());
}

#[test]
fn the_upstream_is_optional_or_a_github_repo() {
    assert_eq!(defaults().upstream().unwrap(), None);
    let config = Config {
        upstream: "https://github.com/acme/shop.git".into(),
        ..defaults()
    };
    let upstream = config.upstream().unwrap().unwrap();
    assert!(
        upstream
            .pull_request("https://github.com/acme/shop/pull/1")
            .is_some()
    );
}

#[test]
fn the_workspace_upstream_and_replicas_are_checked() {
    for bad in [
        Config {
            repo: "shop.git".into(),
            ..defaults()
        },
        Config {
            member: String::new(),
            ..defaults()
        },
        Config {
            stream_replicas: 0,
            ..defaults()
        },
        Config {
            upstream: "https://gitlab.com/acme/shop".into(),
            ..defaults()
        },
    ] {
        assert!(bad.validate().is_err());
    }
}

#[test]
fn nats_tokens_and_issuers_are_checked() {
    let plain_remote = Config {
        nats_url: "nats://nats.example.com:4222".into(),
        ..defaults()
    };
    assert!(
        plain_remote.validate().is_err(),
        "a token never travels in clear text"
    );
    let with = |change: &dyn Fn(&mut NatsAuth)| {
        let mut config = defaults();
        change(&mut config.nats_auth);
        config.validate()
    };
    assert!(with(&|a| a.account.clear()).is_err());
    assert!(
        with(&|a| a.lane_account = a.account.clone()).is_err(),
        "lanes apart"
    );
    assert!(
        with(&|a| a.issuers.clear()).is_err(),
        "one control-plane issuer"
    );
    assert!(
        with(&|a| a.issuers.push(a.issuers[0].clone())).is_err(),
        "two control-plane issuers, one url twice"
    );
    assert!(with(&|a| a.issuers[0].jwks_url = "http://jwks".into()).is_err());
    assert!(with(&|a| a.issuers[0].url = "http://issuer".into()).is_err());
    assert!(with(&|a| a.issuers[0].runner = "a.b".into()).is_err());
    for account in ["runner", ":runner", "ns:", "ns:a:b"] {
        let bad = with(&|a| a.issuers[0].runner_service_account = account.into());
        assert!(bad.is_err(), "{account}");
    }
    let mut remote = defaults().nats_auth.issuers[0].clone();
    remote.url = "https://oidc.eks.example.com/id/1".into();
    remote.control_plane = false;
    assert!(with(&|a| a.issuers.push(remote.clone())).is_ok());
}

fn roots_from(pem: &str) -> Result<Vec<CertificateDer<'static>>, ConfigError> {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    std::io::Write::write_all(&mut file, pem.as_bytes()).unwrap();
    Config {
        extra_ca_file: Some(file.path().to_owned()),
        ..defaults()
    }
    .extra_roots()
}

#[tokio::test]
async fn the_extra_ca_file_is_every_certificate_in_it() {
    assert!(defaults().extra_roots().unwrap().is_empty(), "unset: none");
    let ca = rl_testkit::HttpsStub::start(|_| String::new())
        .await
        .ca_pem
        .clone();
    assert_eq!(roots_from(&[ca.as_str(), &ca].concat()).unwrap().len(), 2);
}

#[test]
fn an_extra_ca_file_that_doesnt_parse_stops_the_api() {
    for pem in [
        "",
        "not a certificate",
        "-----BEGIN CERTIFICATE-----\n!!!!\n-----END CERTIFICATE-----\n",
        "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n",
    ] {
        let error = roots_from(pem).unwrap_err().to_string();
        assert!(error.contains("extra_ca_file /"), "{pem:?}: {error}");
    }
    let missing = Config {
        extra_ca_file: Some("/nonexistent/ca.crt".into()),
        ..defaults()
    };
    let error = missing.extra_roots().unwrap_err().to_string();
    assert!(error.contains("/nonexistent/ca.crt"), "{error}");
}

#[test]
fn the_linear_key_only_travels_over_https() {
    let plain = Config {
        linear_url: "http://api.linear.app/graphql".into(),
        ..defaults()
    };
    assert!(plain.validate().is_err());
}
