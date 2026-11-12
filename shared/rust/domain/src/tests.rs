#![allow(clippy::unwrap_used, clippy::panic)]

use super::*;

const BRIEF: &str = "- AC1 It works.";

fn ticket(key: &str) -> Result<Ticket, DomainError> {
    Ticket::new(
        Tracker::Linear,
        "id-1".into(),
        key.into(),
        "Title".into(),
        BRIEF.into(),
    )
}

#[test]
fn ticket_url_must_be_a_linear_page_and_bounded() {
    let with_url = |url: String| ticket("RAN-1").unwrap().with_url(url);
    for url in [
        String::new(),
        "http://linear.app/x".into(),
        "javascript:alert(1)".into(),
        "https://".into(),
        "https:// x".into(),
        "https://:".into(),
        "https://.".into(),
        "https://:443/x".into(),
        "https://user@evil.example/".into(),
        "https://linear.app/x y".into(),
        "https://linear.app/".into(),
        "https://evil.example/issue/RAN-1".into(),
        "https://linear.app.evil.example/x".into(),
        "https://linear.app:8443/x".into(),
        format!("https://linear.app/{}", "a".repeat(2030)),
    ] {
        assert_eq!(
            with_url(url.clone()),
            Err(DomainError::InvalidTicket("url")),
            "{url:?}"
        );
    }
    let ticket = with_url("https://linear.app/x/issue/RAN-1".into()).unwrap();
    assert_eq!(ticket.url(), "https://linear.app/x/issue/RAN-1");
}

fn workspace() -> Workspace {
    Workspace::new("shop".into(), "main".into(), "ariel".into()).unwrap()
}

#[test]
fn state_machine_allows_hibernate_wake_and_teardown() {
    assert_eq!(State::Hot.change_to(State::Warm), Ok(State::Warm));
    assert_eq!(State::Warm.change_to(State::Hot), Ok(State::Hot));
    assert_eq!(State::Hot.change_to(State::Cold), Ok(State::Cold));
    assert_eq!(State::Warm.change_to(State::Cold), Ok(State::Cold));
}

#[test]
fn state_machine_rejects_no_op_and_leaving_cold() {
    for (from, to) in [
        (State::Hot, State::Hot),
        (State::Cold, State::Hot),
        (State::Cold, State::Warm),
    ] {
        assert_eq!(
            from.change_to(to),
            Err(DomainError::Transition { from, to })
        );
    }
}

#[test]
fn state_names_round_trip_and_unknown_fails() {
    for state in [State::Hot, State::Warm, State::Cold] {
        assert_eq!(State::parse(state.as_str()), Ok(state));
    }
    assert!(State::parse("lukewarm").is_err());
}

#[test]
fn ticket_rejects_keys_that_cant_name_a_namespace() {
    let long = format!("R{}-1", "A".repeat(38));
    for key in [
        "", "-RAN", "RAN-", "RAN 1", "RAN.1", "R/1", "ran-1", "RAN-0", "RAN-01", "1RAN-1",
        "RAN-1-2", &long,
    ] {
        assert_eq!(
            ticket(key),
            Err(DomainError::InvalidTicket("key")),
            "{key:?}"
        );
    }
    let made = |id: &str, title: String, description: String| {
        Ticket::new(
            Tracker::Linear,
            id.into(),
            "RAN-1".into(),
            title,
            description,
        )
    };
    assert_eq!(
        made("", "t".into(), BRIEF.into()),
        Err(DomainError::InvalidTicket("id"))
    );
    assert_eq!(
        made("i", "é".repeat(301), BRIEF.into()),
        Err(DomainError::InvalidTicket("title"))
    );
    assert!(
        made("i", "é".repeat(300), BRIEF.into()).is_ok(),
        "chars, not bytes"
    );
    assert_eq!(
        made("i", "t".into(), "no criteria".into()),
        Err(DomainError::InvalidTicket("criteria"))
    );
    let long = format!("{BRIEF}\n{}", "d".repeat(20_000));
    assert_eq!(
        made("i", "t".into(), long),
        Err(DomainError::InvalidTicket("description"))
    );
}

#[test]
fn criteria_are_the_ac_lines_of_the_description_with_their_continuations() {
    let description = "Intro\n\nAcceptance\n- AC1 first,\n  wrapped\n* **AC2** bold\nAC3 bare\n  more\n\n  not AC3\nACME is not one\n+ AC10 ten";
    assert_eq!(
        ticket::criteria(description),
        [
            "AC1 first, wrapped",
            "AC2 bold",
            "AC3 bare more",
            "AC10 ten"
        ]
    );
    assert_eq!(ticket::criteria(""), Vec::<String>::new());
}

/// AC6: the rule's limits are the schema's, and the schema's examples pass or fail here as they
/// do in `scripts/check.sh spec`.
#[test]
fn the_ticket_rule_is_the_schema() {
    let schema: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../api/jsonschema/ticket.v1alpha1.schema.json"
    ))
    .unwrap();
    let field = |name: &str, key: &str| schema["properties"][name][key].as_u64().unwrap();
    let limits = [
        (field("key", "maxLength"), ticket::KEY_MAX),
        (field("title", "maxLength"), ticket::TITLE_MAX),
        (field("description", "maxLength"), ticket::DESCRIPTION_MAX),
        (field("criteria", "maxItems"), ticket::CRITERIA_MAX),
        (
            schema["properties"]["criteria"]["items"]["maxLength"]
                .as_u64()
                .unwrap(),
            ticket::CRITERION_MAX,
        ),
    ];
    for (schema_max, rust_max) in limits {
        assert_eq!(schema_max, u64::try_from(rust_max).unwrap());
    }
    assert_eq!(
        schema["properties"]["key"]["pattern"], "^[A-Z][A-Z0-9]*-[1-9][0-9]*$",
        "the key check in ticket::check follows this pattern"
    );
    let examples = [
        (
            include_str!("../../../../api/examples/ticket.example.json"),
            true,
        ),
        (
            include_str!("../../../../api/examples/invalid/ticket.key-not-a-tracker-key.json"),
            false,
        ),
        (
            include_str!("../../../../api/examples/invalid/ticket.no-criteria.json"),
            false,
        ),
    ];
    for (example, valid) in examples {
        let t: serde_json::Value = serde_json::from_str(example).unwrap();
        let text = |k: &str| t[k].as_str().unwrap();
        let criteria: Vec<String> = t["criteria"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_str().unwrap().to_owned())
            .collect();
        let checked = ticket::check(text("key"), text("title"), text("description"), &criteria);
        assert_eq!(checked.is_ok(), valid, "{example}");
        if valid {
            assert_eq!(ticket::criteria(text("description")), criteria);
        }
    }
}

/// Review: the workspace rules are the Session CRD's. `test/workspace.cases.json` is the one table
/// of cases; the runner's envtest checks the same table against the CRD on a real API server.
#[test]
fn workspaces_hold_the_session_crd_rules() {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("../../../../test/workspace.cases.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let field = |k: &str| case[k].as_str().unwrap().to_owned();
        let made = Workspace::new(field("repo"), field("baseRef"), field("member"));
        assert_eq!(made.is_ok(), case["valid"].as_bool().unwrap(), "{case}");
    }
}

#[test]
fn phase_names_round_trip_and_reports_cap_their_message() {
    for phase in Phase::ALL {
        assert_eq!(Phase::parse(phase.as_str()), Ok(phase));
    }
    assert!(Phase::parse("hot").is_err());
    let report = ReportPhase {
        session: SessionId(uuid::Uuid::nil()),
        event: EventId(uuid::Uuid::nil()),
        at: std::time::SystemTime::UNIX_EPOCH,
        message_id: uuid::Uuid::nil(),
        phase: Phase::Failed,
        message: "é".repeat(PHASE_MESSAGE_MAX + 1),
        observed_generation: 3,
        sent_at: None,
        pull_request: None,
    };
    let Change::PhaseChanged { message, .. } = report.change("shop", None) else {
        panic!("not a phase change")
    };
    assert_eq!(message.chars().count(), PHASE_MESSAGE_MAX);
    assert_eq!(report.change("shop", None).state(), None);
}

#[test]
fn an_upstream_is_a_github_repo() {
    for url in [
        "https://github.com/acme/shop",
        "https://github.com/acme/shop.git",
        "https://github.com/a-1/s_h.o-p",
    ] {
        assert!(Upstream::new("shop", url).is_ok(), "{url}");
    }
    for url in [
        "",
        "http://github.com/acme/shop",
        "https://gitlab.com/acme/shop",
        "https://github.com/acme",
        "https://github.com/acme/",
        "https://github.com/acme/shop/",
        "https://github.com/acme/shop/x",
        "https://github.com.evil.example/acme/shop",
        "https://github.com/ac me/shop",
    ] {
        assert_eq!(
            Upstream::new("shop", url),
            Err(DomainError::InvalidUpstream),
            "{url:?}"
        );
    }
}

#[test]
fn only_a_pull_request_of_the_upstream_is_one() {
    let upstream = Upstream::new("shop", "https://github.com/acme/shop.git").unwrap();
    let pr = upstream
        .pull_request("https://github.com/Acme/Shop/pull/42")
        .unwrap();
    assert_eq!(
        (pr.repo(), pr.url()),
        ("shop", "https://github.com/Acme/Shop/pull/42")
    );
    for url in [
        "",
        "https://github.com/acme/shop/pull/",
        "https://github.com/acme/shop/pull/0",
        "https://github.com/acme/shop/pull/042",
        "https://github.com/acme/shop/pull/42/files",
        "https://github.com/acme/shop/pull/42?x=1",
        "https://github.com/acme/shop/pull/12345678901",
        "https://github.com/acme/shopx/pull/42",
        "https://github.com/evil/shop/pull/42",
        "https://github.com/acme/shop/issues/42",
        "http://github.com/acme/shop/pull/42",
        "https://github.com/acme/shé/pull/42",
        // The database's check is case-sensitive: so is this, for all but owner and repo.
        "HTTPS://github.com/acme/shop/pull/42",
        "https://GitHub.com/acme/shop/pull/42",
        "https://github.com/acme/shop/PULL/42",
    ] {
        assert_eq!(upstream.pull_request(url), None, "{url:?}");
    }
}

#[test]
fn a_session_keeps_the_first_pull_request_of_its_own_repo() {
    let upstream = Upstream::new("shop", "https://github.com/acme/shop").unwrap();
    let report = ReportPhase {
        session: SessionId(uuid::Uuid::nil()),
        event: EventId(uuid::Uuid::nil()),
        at: std::time::SystemTime::UNIX_EPOCH,
        message_id: uuid::Uuid::nil(),
        phase: Phase::Running,
        message: String::new(),
        observed_generation: 1,
        sent_at: None,
        pull_request: upstream.pull_request("https://github.com/acme/shop/pull/7"),
    };
    let recorded = |repo: &str, recorded: Option<&str>| match report.change(repo, recorded) {
        Change::PhaseChanged {
            pull_request_url, ..
        } => pull_request_url,
        _ => panic!("not a phase change"),
    };
    assert_eq!(
        recorded("shop", None).as_deref(),
        Some("https://github.com/acme/shop/pull/7")
    );
    assert_eq!(recorded("other", None), None, "another repo's session");
    assert_eq!(
        recorded("shop", Some("https://github.com/acme/shop/pull/1")),
        None,
        "it has one"
    );
}

#[test]
fn new_session_is_hot_in_its_ticket_namespace() {
    let command = NewSession {
        id: SessionId(uuid::Uuid::nil()),
        event: EventId(uuid::Uuid::nil()),
        at: std::time::SystemTime::UNIX_EPOCH,
        ticket: ticket("RAN-34").unwrap(),
        workspace: workspace(),
    };
    assert_eq!(command.namespace(), "rl-ran-34");
    assert_eq!(command.change().state(), Some(State::Hot));
    assert_eq!(command.change().kind(), "session.created");
}

#[test]
fn subject_tokens_cant_widen_a_subject() {
    assert_eq!(subject::token("acme-1"), Ok("acme-1"));
    for bad in ["", "a.b", "*", ">", "a b", "a\n"] {
        assert!(subject::token(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn a_member_is_the_lowercased_login_or_nothing() {
    assert_eq!(
        Member::from_username("ArielLahiany").unwrap().as_str(),
        "ariellahiany"
    );
    assert_eq!(Member::from_username("a-1").unwrap().as_str(), "a-1");
    assert_eq!(
        Member::from_username(&"a".repeat(63))
            .unwrap()
            .as_str()
            .len(),
        63
    );
    for bad in [
        "",
        "-a",
        "a-",
        "a.b",
        "a_b",
        "a@example.com",
        "a b",
        "ı",
        &"a".repeat(64),
    ] {
        assert_eq!(
            Member::from_username(bad),
            Err(DomainError::InvalidMember),
            "{bad:?}"
        );
    }
}

/// Every identity's permissions are exactly the ones `test/nats-permissions.json` pins, which the
/// runner's Go protocol tests run under too.
#[test]
fn nats_permissions_are_the_pinned_ones() {
    let pinned: serde_json::Value =
        serde_json::from_str(include_str!("../../../../test/nats-permissions.json")).unwrap();
    let org = pinned["org"].as_str().unwrap();
    let runner = pinned["runner_id"].as_str().unwrap();
    let session = SessionId(pinned["session"].as_str().unwrap().parse().unwrap());
    let json = |p: subject::Permissions| serde_json::json!({"publish": p.publish, "subscribe": p.subscribe});
    assert_eq!(json(subject::api(org)), pinned["api"]);
    assert_eq!(json(subject::relay(org)), pinned["relay"]);
    assert_eq!(json(subject::runner(org, runner)), pinned["runner"]);
    assert_eq!(json(subject::lane(org, session)), pinned["lane"]);
}

/// RAN-43 review 8: no identity may publish to `$SYS`, so none can answer the server's sign-in
/// requests on `$SYS.REQ.USER.AUTH` (config mode doesn't verify who signed a callout's request,
/// only who answers it). A subject that can reach `$SYS` starts with it, or with a wildcard.
#[test]
fn no_identity_may_publish_to_sys() {
    let pinned: serde_json::Value =
        serde_json::from_str(include_str!("../../../../test/nats-permissions.json")).unwrap();
    for identity in ["api", "relay", "runner", "lane"] {
        for subject in pinned[identity]["publish"].as_array().unwrap() {
            let first = subject.as_str().unwrap().split('.').next().unwrap();
            assert!(
                !matches!(first, "$SYS" | "*" | ">"),
                "{identity} may publish {subject}"
            );
        }
    }
}
