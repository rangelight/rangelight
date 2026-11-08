use crate::DomainError;

/// A member: who is signed in to the Workbench, and the value of a session's `member` label
/// (RAN-19), so `^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$`.
///
/// It comes from the ID token's `preferred_username` (the GitHub login, through Dex), lowercased:
/// GitHub logins are case-insensitive, and their characters (letters, digits, single inner `-`,
/// at most 39) always fit the label. A username that doesn't fit after lowercasing (an `@`, `.`
/// or `_` from another provider) is refused, never rewritten, so two accounts can't map to one
/// member. Which account owns a member is the store's business (`members`, bound at first sign-in).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Member(String);

impl Member {
    /// The member for an OIDC `preferred_username`.
    ///
    /// # Errors
    /// [`DomainError::InvalidMember`] if, lowercased, it isn't a label value.
    pub fn from_username(username: &str) -> Result<Member, DomainError> {
        let name = username.to_ascii_lowercase();
        let bytes = name.as_bytes();
        let ok = (1..=63).contains(&bytes.len())
            && bytes
                .iter()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
            && bytes.first() != Some(&b'-')
            && bytes.last() != Some(&b'-');
        if !ok {
            return Err(DomainError::InvalidMember);
        }
        Ok(Member(name))
    }

    /// The label value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
