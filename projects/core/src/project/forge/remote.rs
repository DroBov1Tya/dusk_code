//! The remote a project's `origin` points to, as a host and a path. Remote text is
//! untrusted: only a DNS host and a plain `owner/repo` path get through.

use std::path::Path;

use crate::project::git;

/// The longest path of a remote: GitLab nests groups.
const SEGMENTS: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    /// Lowercased DNS name with a dot, never an IP address or `localhost`.
    pub host: String,
    /// `owner/repo`, 1 to 4 segments.
    pub path: String,
}

impl Remote {
    /// The remote `origin` of the repository in `dir`; `None` when it has none or it
    /// is not a web-hosted one.
    pub async fn of(dir: &Path) -> Option<Remote> {
        let remote = git::git(dir, &["remote", "get-url", "origin"]).await.ok()?;
        Remote::parse(remote.trim())
    }

    pub fn parse(remote: &str) -> Option<Remote> {
        let page = git::web_page(remote)?;
        let (host, path) = page.strip_prefix("https://")?.split_once('/')?;
        let host = host.to_ascii_lowercase();
        // An address names no forge; its last label is never all digits in a DNS name.
        let address = host.rsplit('.').next()?.bytes().all(|b| b.is_ascii_digit());
        let segment = |part: &str| {
            !part.is_empty()
                && part.len() <= 100
                && !part.starts_with('.')
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
        };
        let fine = !address
            && host.contains('.')
            && path.split('/').count() <= SEGMENTS
            && path.split('/').all(segment);
        fine.then(|| Remote {
            host,
            path: path.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Remote;

    #[test]
    fn a_remote_is_a_dns_host_and_a_safe_path() {
        let r = |s| Remote::parse(s).map(|r| (r.host, r.path));
        let own = Some(("github.com".to_owned(), "owner/repo".to_owned()));
        assert_eq!(r("git@GitHub.com:owner/repo.git"), own);
        assert_eq!(r("ssh://git@github.com:22/owner/repo"), own);
        assert_eq!(r("https://user:token@github.com/owner/repo.git/"), own);
        assert_eq!(
            r("https://gitlab.com/a/b/c/d").map(|r| r.1),
            Some("a/b/c/d".to_owned())
        );
        for bad in [
            // An `@` in the path never changes the host.
            "https://evil.com/a@github.com/o/r",
            "ssh://evil.com/a@github.com/o/r",
            "https://gitlab.com/a/b/c/d/e",
            "https://localhost/o/r",
            "https://10.0.0.1/o/r",
            "https://github.com/.hidden/r",
            "https://github.com/o//r",
            "https://github.com/o/r%20x",
            &format!("https://github.com/{}/r", "a".repeat(101)),
            "file:///tmp/r",
            "",
        ] {
            assert_eq!(r(bad), None, "{bad}");
        }
    }
}
