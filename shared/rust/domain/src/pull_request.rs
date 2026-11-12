use crate::DomainError;

const GITHUB: &str = "https://github.com/";

/// Where a git mirror repo comes from on GitHub, `https://github.com/<owner>/<repo>`: the only
/// place a lane's pull request may be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upstream {
    repo: String,
    /// `<owner>/<repo>`.
    path: String,
}

impl Upstream {
    /// The upstream of the mirror repo `repo`, from its URL (with or without `.git`).
    ///
    /// # Errors
    /// [`DomainError::InvalidUpstream`] unless `url` is `https://github.com/<owner>/<repo>`.
    pub fn new(repo: &str, url: &str) -> Result<Upstream, DomainError> {
        let path = url
            .strip_prefix(GITHUB)
            .ok_or(DomainError::InvalidUpstream)?;
        let path = path.strip_suffix(".git").unwrap_or(path);
        let segment = |s: &str| {
            (1..=100).contains(&s.len())
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        };
        match path.split_once('/') {
            Some((owner, name)) if segment(owner) && segment(name) => Ok(Upstream {
                repo: repo.to_owned(),
                path: path.to_owned(),
            }),
            _ => Err(DomainError::InvalidUpstream),
        }
    }

    /// `url` as a pull request of this upstream, if it is `https://github.com/<owner>/<repo>/pull/<n>`:
    /// the scheme, host and `/pull/` exactly (as `sessions_pull_request_url_github` checks them),
    /// owner and repo in any case (as GitHub matches them).
    #[must_use]
    pub fn pull_request(&self, url: &str) -> Option<PullRequest> {
        let rest = url.strip_prefix(GITHUB)?;
        let (path, rest) = rest.split_at_checked(self.path.len())?;
        let number = rest.strip_prefix("/pull/")?;
        let number_ok = (1..=10).contains(&number.len())
            && !number.starts_with('0')
            && number.bytes().all(|b| b.is_ascii_digit());
        (path.eq_ignore_ascii_case(&self.path) && number_ok).then(|| PullRequest {
            repo: self.repo.clone(),
            url: url.to_owned(),
        })
    }
}

/// A pull request a runner reported, on the upstream of the mirror repo it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    repo: String,
    url: String,
}

impl PullRequest {
    /// The mirror repo whose upstream it is on.
    #[must_use]
    pub fn repo(&self) -> &str {
        &self.repo
    }

    /// Its page, `https://github.com/<owner>/<repo>/pull/<n>`.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }
}
