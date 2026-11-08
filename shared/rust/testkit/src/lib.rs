//! Test support (ADR 0049 §2.2, §8): fakes for the seams, the `TicketSource` contract suite, a
//! Linear stub, and real Postgres, NATS and Dex. A dev-dependency only.
//!
//! Postgres, NATS and Dex come from `scripts/test/services.sh run`, which sets `DATABASE_URL`
//! (read by sqlx) and `RANGELIGHT_TESTKIT_*`.
#![deny(missing_docs)]
// Test support: failing loudly is the point, so any function here may panic.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc
)]

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use rl_app::{Clock, IdGen, TicketError, TicketSource};
use rl_domain::{INTAKE_LABEL, Ticket, Tracker, Workspace};
use sqlx::Postgres;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::testing::{TestArgs, TestSupport};
use uuid::Uuid;

mod browser;
mod dex;
mod linear_stub;

pub use browser::{Browser, Page};
pub use dex::{Dex, PUBLIC_URL, dex, sign_in_at_dex};
pub use linear_stub::{LinearStub, Reply, StubRequest};

/// A ticket as a tracker holds it, with its labels: what a test seeds a source with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seed {
    /// The ticket.
    pub ticket: Ticket,
    /// Its label names.
    pub labels: Vec<String>,
}

impl Seed {
    /// Whether it carries [`INTAKE_LABEL`]: what a tracker filters on.
    #[must_use]
    pub fn labelled(&self) -> bool {
        self.labels.iter().any(|label| label == INTAKE_LABEL)
    }
}

/// A [`TicketSource`] over seeded tickets; like a tracker, it lists only the labelled ones.
#[derive(Debug, Clone, Default)]
pub struct FakeTickets(pub Vec<Seed>);

impl TicketSource for FakeTickets {
    fn tickets(&self) -> impl Future<Output = Result<Vec<Ticket>, TicketError>> + Send {
        let labelled = self
            .0
            .iter()
            .filter(|seed| seed.labelled())
            .map(|seed| seed.ticket.clone());
        std::future::ready(Ok(labelled.collect()))
    }
}

/// A [`Clock`] stopped at one time.
#[derive(Debug, Clone, Copy)]
pub struct FixedClock(pub SystemTime);

impl Clock for FixedClock {
    fn now(&self) -> SystemTime {
        self.0
    }
}

/// An [`IdGen`] counting up from 1, offset by `base` so tests sharing a database don't collide.
#[derive(Debug, Default)]
pub struct SeqIds {
    /// Added to every id.
    pub base: u128,
    next: AtomicU64,
}

impl IdGen for SeqIds {
    fn new_id(&self) -> Uuid {
        Uuid::from_u128(self.base + u128::from(self.next.fetch_add(1, Ordering::Relaxed) + 1))
    }
}

/// A seeded Linear ticket for tests: id `issue-<n>`, key `RAN-<n>`, two acceptance criteria.
#[must_use]
pub fn ticket(n: u32, labels: &[&str]) -> Seed {
    let ticket = Ticket::new(
        Tracker::Linear,
        format!("issue-{n}"),
        format!("RAN-{n}"),
        format!("Ticket {n}"),
        format!("Ticket {n}.\n\nAcceptance\n- AC1 one\n- AC2 two"),
    )
    .unwrap();
    Seed {
        ticket,
        labels: labels.iter().map(|label| (*label).to_owned()).collect(),
    }
}

/// The contract every [`TicketSource`] passes (ADR 0049 §2.2). `make` builds a source over
/// exactly the given seeds. A source lists every ticket carrying [`INTAKE_LABEL`] once, across as
/// many pages as it takes, and no other.
pub async fn ticket_source_contract<S, F, Fut>(make: F)
where
    S: TicketSource,
    F: Fn(Vec<Seed>) -> Fut,
    Fut: Future<Output = S>,
{
    assert!(
        make(Vec::new()).await.tickets().await.unwrap().is_empty(),
        "an empty source lists nothing"
    );

    let seeds: Vec<Seed> = (1..=180)
        .map(|n| {
            if n % 3 == 0 {
                ticket(n, &["bug"])
            } else {
                ticket(n, &["bug", INTAKE_LABEL])
            }
        })
        .collect();
    let mut listed = make(seeds.clone()).await.tickets().await.unwrap();
    listed.sort_by(|a, b| a.id().cmp(b.id()));
    let mut expected: Vec<Ticket> = seeds
        .into_iter()
        .filter(Seed::labelled)
        .map(|seed| seed.ticket)
        .collect();
    expected.sort_by(|a, b| a.id().cmp(b.id()));
    assert_eq!(
        listed, expected,
        "a source lists every labelled ticket once, and nothing else"
    );
}

/// The workspace tests start sessions in: repo `shop` at `main`, for member `ariel`.
#[must_use]
pub fn workspace() -> Workspace {
    Workspace::new("shop".into(), "main".into(), "ariel".into()).unwrap()
}

/// A client of the test NATS server, with JetStream.
pub async fn nats() -> async_nats::Client {
    #[derive(serde::Deserialize)]
    struct Env {
        nats_url: String,
    }
    let env: Env = rl_config::load("testkit", "nats_url = \"nats://127.0.0.1:4222\"").unwrap();
    async_nats::connect(&env.nats_url)
        .await
        .expect("NATS: run the tests via scripts/test/services.sh")
}

/// A fresh, migrated database of its own, named after `test` (unique per test).
#[derive(Debug, Clone)]
pub struct Database {
    options: PgConnectOptions,
}

impl Database {
    /// Creates and migrates it.
    pub async fn new(test: &'static str) -> Database {
        let args = TestArgs {
            test_path: test,
            migrator: Some(&rl_store::MIGRATOR),
            fixtures: &[],
        };
        let context = Postgres::test_context(&args)
            .await
            .expect("Postgres: run the tests via scripts/test/services.sh");
        let database = Database {
            options: context.connect_opts,
        };
        database.store().await.migrate().await.unwrap();
        database
    }

    /// A store on it, connected as production connects.
    pub async fn store(&self) -> rl_store::Store {
        rl_store::Store::connect_with(self.options.clone())
            .await
            .unwrap()
    }

    /// Runs a fixture: SQL from a file of the test, never a Rust string (ADR 0049 §2.4).
    pub async fn execute(&self, sql: &'static str) {
        self.try_execute(sql).await.unwrap();
    }

    /// Runs a fixture that may fail.
    ///
    /// # Errors
    /// Postgres's message.
    pub async fn try_execute(&self, sql: &'static str) -> Result<(), String> {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(self.options.clone())
            .await
            .unwrap();
        let result = sqlx::raw_sql(sql)
            .execute(&pool)
            .await
            .map(drop)
            .map_err(|e| e.to_string());
        pool.close().await;
        result
    }
}

impl Database {
    /// Runs a fixture in a transaction left open, so its row locks are held until the returned
    /// transaction is dropped.
    pub async fn hold(&self, sql: &'static str) -> sqlx::Transaction<'static, Postgres> {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(self.options.clone())
            .await
            .unwrap();
        let mut transaction = pool.begin().await.unwrap();
        sqlx::raw_sql(sql).execute(&mut *transaction).await.unwrap();
        transaction
    }
}

/// A store on a fresh, migrated database of its own, named after `test` (unique per test).
pub async fn store(test: &'static str) -> rl_store::Store {
    Database::new(test).await.store().await
}
