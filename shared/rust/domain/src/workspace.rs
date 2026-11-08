use crate::{DomainError, Member};

/// Where a session works, and for whom: the runner's `Session` spec takes these as they are
/// (`deployments/helm/rangelight/files/crds/session.yaml` holds the same rules).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    repo: String,
    base_ref: String,
    member: String,
}

impl Workspace {
    /// A workspace.
    ///
    /// # Errors
    /// [`DomainError::InvalidWorkspace`] when `repo` isn't a git mirror repo name (1 to 100 of
    /// `A-Za-z0-9._-`, starting alphanumeric, without `.git`), `base_ref` isn't a branch name, or
    /// `member` isn't a DNS label.
    pub fn new(repo: String, base_ref: String, member: String) -> Result<Workspace, DomainError> {
        let name = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-');
        let repo_ok = (1..=100).contains(&repo.len())
            && repo.starts_with(|c: char| c.is_ascii_alphanumeric())
            && repo.bytes().all(name)
            && repo.strip_suffix(".git").is_none();
        if !repo_ok {
            return Err(DomainError::InvalidWorkspace("repo"));
        }
        let ref_ok = (1..=200).contains(&base_ref.len())
            && base_ref.starts_with(|c: char| c.is_ascii_alphanumeric())
            && base_ref.bytes().all(|b| name(b) || b == b'/')
            && !["..", "//"].iter().any(|bad| base_ref.contains(bad))
            && !base_ref.ends_with('/')
            && base_ref.strip_suffix(".lock").is_none();
        if !ref_ok {
            return Err(DomainError::InvalidWorkspace("base_ref"));
        }
        // The member label rule is Member's; here it must already be the label, not a username to
        // lowercase.
        let member_ok = Member::from_username(&member).is_ok_and(|m| m.as_str() == member);
        if !member_ok {
            return Err(DomainError::InvalidWorkspace("member"));
        }
        Ok(Workspace {
            repo,
            base_ref,
            member,
        })
    }

    /// The git mirror's name for the repo, served at `/<repo>.git`.
    #[must_use]
    pub fn repo(&self) -> &str {
        &self.repo
    }

    /// The approved base branch: the worktree starts there, and the profile is read there.
    #[must_use]
    pub fn base_ref(&self) -> &str {
        &self.base_ref
    }

    /// Whose Claude home the lane mounts (ADR 0046 amendment).
    #[must_use]
    pub fn member(&self) -> &str {
        &self.member
    }
}
