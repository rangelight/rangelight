use crate::DomainError;

/// The label that asks Rangelight for a session.
pub const INTAKE_LABEL: &str = "rangelight";

/// The ticket rule's limits, in characters (`api/jsonschema/ticket.v1alpha1.schema.json`; a test
/// holds these to the schema). The key is also `^[A-Z][A-Z0-9]*-[1-9][0-9]*$`.
pub(crate) const KEY_MAX: usize = 40;
pub(crate) const TITLE_MAX: usize = 300;
pub(crate) const DESCRIPTION_MAX: usize = 20_000;
pub(crate) const CRITERIA_MAX: usize = 50;
pub(crate) const CRITERION_MAX: usize = 2000;

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
/// through [`Ticket::new`], which holds it to the ticket rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    tracker: Tracker,
    id: String,
    key: String,
    title: String,
    description: String,
    criteria: Vec<String>,
    url: String,
}

impl Ticket {
    /// A ticket from a tracker. Its acceptance criteria are read from the description: each line
    /// that starts with `AC<n>`, with the indented lines that follow it.
    ///
    /// # Errors
    /// [`DomainError::InvalidTicket`] when the id is empty or over 128 bytes, or the ticket breaks
    /// the rule of `api/jsonschema/ticket.v1alpha1.schema.json`.
    pub fn new(
        tracker: Tracker,
        id: String,
        key: String,
        title: String,
        description: String,
    ) -> Result<Ticket, DomainError> {
        if id.is_empty() || id.len() > 128 {
            return Err(DomainError::InvalidTicket("id"));
        }
        let criteria = criteria(&description);
        check(&key, &title, &description, &criteria)?;
        Ok(Ticket {
            tracker,
            id,
            key,
            title,
            description,
            criteria,
            url: String::new(),
        })
    }

    /// The ticket with its page in the tracker, which the Workbench links to.
    ///
    /// # Errors
    /// [`DomainError::InvalidTicket`] when `url` isn't `https://<host>` (see [`https_url`]).
    pub fn with_url(self, url: String) -> Result<Ticket, DomainError> {
        if !https_url(&url) {
            return Err(DomainError::InvalidTicket("url"));
        }
        Ok(Ticket { url, ..self })
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

    /// The description, the lane's brief.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// The acceptance criteria, in order.
    #[must_use]
    pub fn criteria(&self) -> &[String] {
        &self.criteria
    }

    /// The ticket's page in its tracker, for example `https://linear.app/<org>/issue/RAN-34`;
    /// empty until [`Ticket::with_url`] sets it.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }
}

/// `https://` with a host (letters, digits, `-`, `.`, an optional `:port`; no userinfo) that has a
/// letter or digit before any `:`, at most 2048 bytes, with no whitespace or control characters.
fn https_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let name = host.split(':').next().unwrap_or_default();
    url.len() <= 2048
        && name.bytes().any(|b| b.is_ascii_alphanumeric())
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b':'))
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// The ticket rule (`api/jsonschema/ticket.v1alpha1.schema.json`), lengths in characters as JSON
/// Schema counts them.
pub(crate) fn check(
    key: &str,
    title: &str,
    description: &str,
    criteria: &[String],
) -> Result<(), DomainError> {
    let within = |s: &str, max: usize| !s.is_empty() && s.chars().count() <= max;
    // `^[A-Z][A-Z0-9]*-[1-9][0-9]*$`: `rl-` plus the key stays a DNS label.
    let key_ok = key.len() <= KEY_MAX
        && key.split_once('-').is_some_and(|(team, number)| {
            team.starts_with(|c: char| c.is_ascii_uppercase())
                && team
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
                && number.starts_with(|c: char| matches!(c, '1'..='9'))
                && number.bytes().all(|b| b.is_ascii_digit())
        });
    if !key_ok {
        return Err(DomainError::InvalidTicket("key"));
    }
    if !within(title, TITLE_MAX) {
        return Err(DomainError::InvalidTicket("title"));
    }
    if description.chars().count() > DESCRIPTION_MAX {
        return Err(DomainError::InvalidTicket("description"));
    }
    if criteria.is_empty()
        || criteria.len() > CRITERIA_MAX
        || !criteria.iter().all(|c| within(c, CRITERION_MAX))
    {
        return Err(DomainError::InvalidTicket("criteria"));
    }
    Ok(())
}

/// The acceptance criteria written in a description: each line that starts with `AC<n>`, after
/// an optional list marker (`-`, `*`, `+`) and an optional bold `**AC<n>**`, plus the indented
/// lines that follow it, joined by spaces. A blank or unindented line ends a criterion.
#[must_use]
pub(crate) fn criteria(description: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut open = false;
    for line in description.lines() {
        let text = line.trim();
        let item = text
            .strip_prefix(['-', '*', '+'])
            .map_or(text, str::trim_start);
        let (item, bold) = item.strip_prefix("**").map_or((item, false), |i| (i, true));
        let starts = item
            .strip_prefix("AC")
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()));
        if starts {
            found.push(if bold {
                item.replacen("**", "", 1)
            } else {
                item.to_owned()
            });
            open = true;
        } else if open && !text.is_empty() && line.starts_with(char::is_whitespace) {
            if let Some(last) = found.last_mut() {
                last.push(' ');
                last.push_str(text);
            }
        } else {
            open = false;
        }
    }
    found
}
