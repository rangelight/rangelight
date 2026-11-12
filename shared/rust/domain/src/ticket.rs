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
    /// [`DomainError::InvalidTicket`] when `url` isn't a Linear page (see [`linear_url`]).
    pub fn with_url(self, url: String) -> Result<Ticket, DomainError> {
        if !linear_url(&url) {
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

/// A page on `https://linear.app/`, at most 2048 bytes, with no whitespace or control characters:
/// the ticket schema's `url` rule, so the runner's Session never refuses it.
fn linear_url(url: &str) -> bool {
    url.len() <= 2048
        && url
            .strip_prefix("https://linear.app/")
            .is_some_and(|path| !path.is_empty())
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

/// The acceptance criteria written in a description, in order: each line that starts with
/// `AC<n>`, and each unindented numbered item (`1.` or `1)`; an indented one is a nested list)
/// under an "Acceptance" or "Acceptance criteria" heading: `#` of any level, or a line wholly in
/// bold. A list marker (`-`, `*`, `+`) and a bold `**AC<n>**` before the text are dropped; a bold
/// line that is a numbered or AC item is that item. The indented lines that follow a criterion are
/// part of it, joined by spaces; a blank or unindented line ends it, and the next heading (`#` or a
/// bold line that is no item, say `**Out of scope**`) ends the section. Lines inside a code fence (three backticks or tildes) are no part of anything.
#[must_use]
pub(crate) fn criteria(description: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let (mut open, mut section, mut fence) = (false, false, None);
    for line in description.lines() {
        let text = line.trim();
        if fenced(text, &mut fence) {
            open = false;
            continue;
        }
        let indented = line.starts_with(char::is_whitespace);
        match Line::of(text) {
            Line::Heading(title) => {
                section = acceptance(title);
                open = false;
            }
            Line::Item { text, ac, numbered } if ac || (numbered && section && !indented) => {
                found.push(text);
                open = true;
            }
            _ if open && !text.is_empty() && indented => {
                if let Some(last) = found.last_mut() {
                    last.push(' ');
                    last.push_str(text);
                }
            }
            Line::Item { .. } => open = false,
        }
    }
    found
}

/// Whether `text` is a code fence's line, its opening and closing ones included; `fence` is the
/// open fence's marker.
fn fenced(text: &str, fence: &mut Option<&'static str>) -> bool {
    if let Some(marker) = *fence {
        if text.starts_with(marker) {
            *fence = None;
        }
        return true;
    }
    *fence = ["```", "~~~"]
        .into_iter()
        .find(|marker| text.starts_with(marker));
    fence.is_some()
}

/// Whether a heading's title is "Acceptance" or "Acceptance criteria", in any case, with an
/// optional colon.
fn acceptance(title: &str) -> bool {
    matches!(
        title.trim_end_matches(':').trim().to_lowercase().as_str(),
        "acceptance" | "acceptance criteria"
    )
}

/// One line of a description, as [`criteria`] reads it.
enum Line<'a> {
    /// A heading, with its title.
    Heading(&'a str),
    /// Anything else: its text without a list marker or a leading bold, whether it starts with
    /// `AC<n>`, and whether it is a numbered item.
    Item {
        text: String,
        ac: bool,
        numbered: bool,
    },
}

impl Line<'_> {
    fn of(text: &str) -> Line<'_> {
        if let Some(title) = heading(text) {
            return Line::Heading(title);
        }
        // A line wholly in bold is an item when its text is one (`**1. first**`, `**AC1 …**`), and
        // else a heading (`**Acceptance criteria**`, `**Out of scope**`).
        if let Some(inner) = bold(text) {
            return match Line::of(inner) {
                item @ (Line::Item { ac: true, .. } | Line::Item { numbered: true, .. }) => item,
                _ => Line::Heading(inner),
            };
        }
        let (item, numbered) = list_item(text);
        let (item, bold) = item.strip_prefix("**").map_or((item, false), |i| (i, true));
        let ac = item
            .strip_prefix("AC")
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()));
        let text = if bold {
            item.replacen("**", "", 1)
        } else {
            item.to_owned()
        };
        Line::Item { text, ac, numbered }
    }
}

/// A list item's text without its marker, and whether the list is numbered; any other line as it is.
fn list_item(text: &str) -> (&str, bool) {
    let digits = text.len() - text.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if (1..=9).contains(&digits)
        && let Some(rest) = text[digits..].strip_prefix(['.', ')'])
        && rest.starts_with(char::is_whitespace)
    {
        return (rest.trim_start(), true);
    }
    // `**` opens bold, not a `*` list.
    if text.starts_with("**") {
        return (text, false);
    }
    (
        text.strip_prefix(['-', '*', '+'])
            .map_or(text, str::trim_start),
        false,
    )
}

/// A `#` heading's title, at any level.
fn heading(text: &str) -> Option<&str> {
    let rest = text.strip_prefix('#')?.trim_start_matches('#');
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
}

/// The text of a line wholly in bold: `**text**`, or `**text:**` as `text:`.
fn bold(text: &str) -> Option<&str> {
    let inner = text.strip_prefix("**")?;
    let inner = inner
        .strip_suffix("**")
        .or_else(|| inner.strip_suffix("**:"))?;
    (!inner.is_empty() && !inner.contains("**")).then_some(inner)
}
