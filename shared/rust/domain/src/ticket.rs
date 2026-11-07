use crate::DomainError;

/// The label that asks Rangelight for a session.
pub const INTAKE_LABEL: &str = "rangelight";

/// The ticket trackers Rangelight reads (ADR 0049 §2.2, `TicketSource`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tracker {
    /// Linear.
    Linear,
}

impl Tracker {
    /// The stored name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Tracker::Linear => "linear",
        }
    }
}

/// A ticket that asks for a session: its tracker lists it with [`INTAKE_LABEL`]. Built only
/// through [`Ticket::new`], which validates what came from outside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    tracker: Tracker,
    id: String,
    key: String,
    title: String,
}

impl Ticket {
    /// A ticket from a tracker.
    ///
    /// # Errors
    /// [`DomainError::InvalidTicket`] when the id is empty or over 128 bytes, the key isn't 1 to
    /// 40 ASCII letters, digits or `-` (it names the namespace), or the title is over 1024 bytes.
    pub fn new(
        tracker: Tracker,
        id: String,
        key: String,
        title: String,
    ) -> Result<Ticket, DomainError> {
        if id.is_empty() || id.len() > 128 {
            return Err(DomainError::InvalidTicket("id"));
        }
        // `rl-` plus the key must stay a DNS-1123 label of at most 63 characters.
        let key_ok = !key.is_empty()
            && key.len() <= 40
            && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !key.starts_with('-')
            && !key.ends_with('-');
        if !key_ok {
            return Err(DomainError::InvalidTicket("key"));
        }
        if title.len() > 1024 {
            return Err(DomainError::InvalidTicket("title"));
        }
        Ok(Ticket {
            tracker,
            id,
            key,
            title,
        })
    }

    /// The tracker it came from.
    #[must_use]
    pub fn tracker(&self) -> Tracker {
        self.tracker
    }

    /// The tracker's id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The human key, for example `RAN-34`.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }
}
