//! Git hosts, as the task's proxy reads them from inside TLS: what a
//! request over git's smart HTTP protocol (and LFS) does, so that a fetch
//! passes once the host is approved while a push waits for a person on its
//! own, for its repository.
//!
//! Deny by default: a request is a read only when its method reads (`GET`,
//! `HEAD`, `OPTIONS`) and it has no body, or when it is git's fetch
//! (`POST …/git-upload-pack`) or an LFS download, each to a path Kule can
//! show plainly. Everything else is held for a person: a push to its
//! repository, any other write to the host. A request Kule could read two
//! ways (a `service` asked twice, a query spelled with escapes) or that
//! names a repository it cannot show plainly is refused.

use crate::approvals::HeldRequest;

/// The hosts Kule reads git's requests to: where a push and a fetch are
/// told apart. A host not here is one decision for its connection.
pub(crate) const GIT_HOSTS: &[&str] = &[
    "github.com",
    "gist.github.com",
    "gitlab.com",
    "bitbucket.org",
    "codeberg.org",
    "gitea.com",
    "dev.azure.com",
];

/// Whether `host` (any case, with or without the final dot) is one of
/// [`GIT_HOSTS`].
pub(crate) fn is_git_host(host: &str) -> bool {
    GIT_HOSTS.contains(&crate::network::normal_host(host).as_str())
}

/// What a request to a git host is, by its method, target and whether it
/// has a body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Placed {
    /// Passes once the host is approved.
    Read,
    /// An LFS batch request for the repository at `repo`: its body says
    /// whether it downloads or uploads (see [`lfs_batch`]).
    LfsBatch { repo: String },
    /// Waits for a person.
    Held(HeldRequest),
    /// Refused, and why, in Kule's words.
    Refused(String),
}

/// Places a request to a git host; `bodiless` when its framing says it
/// has no body.
pub(crate) fn place(method: &str, target: &str, bodiless: bool) -> Placed {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let reads = matches!(method, "GET" | "HEAD" | "OPTIONS");
    if let Some(repo) = path.strip_suffix("/info/refs") {
        // Which side of git a client asks for: escapes or a second
        // `service` could be read one way here and another by the host.
        if query.contains(['%', '+']) {
            return Placed::Refused(
                "it asks for git's refs with a query Kule could read two ways".to_owned(),
            );
        }
        let services: Vec<&str> = query
            .split('&')
            .filter_map(|p| {
                let (name, value) = p.split_once('=').unwrap_or((p, ""));
                (name == "service").then_some(value)
            })
            .collect();
        return match services[..] {
            ["git-receive-pack"] => push(repo),
            [] | ["git-upload-pack"] if reads && bodiless => Placed::Read,
            [] | ["git-upload-pack"] => Placed::Held(HeldRequest::Write),
            [_] => Placed::Refused("it asks for a git service Kule does not know".to_owned()),
            _ => Placed::Refused("it asks for git's refs for two services at once".to_owned()),
        };
    }
    if let Some(repo) = path.strip_suffix("/git-receive-pack") {
        return push(repo);
    }
    if let Some((repo, rest)) = path.split_once("/info/lfs/") {
        if method == "POST" && rest == "objects/batch" {
            return match repo_of(repo) {
                Ok(repo) => Placed::LfsBatch { repo },
                Err(why) => Placed::Refused(why),
            };
        }
        // Locks, verification: what an LFS push sends, a write to the
        // repository like the push itself; reading the locks is a read.
        if !(reads && bodiless) {
            return push(repo);
        }
    }
    if reads && bodiless {
        return Placed::Read;
    }
    if method == "POST"
        && let Some(repo) = path.strip_suffix("/git-upload-pack")
        && repo_of(repo).is_ok()
    {
        return Placed::Read;
    }
    Placed::Held(HeldRequest::Write)
}

/// A push to the repository at `path`, or refused when it cannot be shown
/// plainly.
fn push(path: &str) -> Placed {
    match repo_of(path) {
        Ok(repo) => Placed::Held(HeldRequest::Push { repo }),
        Err(why) => Placed::Refused(why),
    }
}

/// The longest repository path Kule shows.
const MAX_REPO: usize = 200;

/// A repository's path on its host as a person is shown it: without the
/// leading `/` and a final `.git`. Only letters, digits and `-._~`
/// between single slashes, and no `.` or `..` part: what a person reads
/// is what the host reads, with no escape, space or control character to
/// make one repository look like another.
pub(crate) fn repo_of(path: &str) -> Result<String, String> {
    let bad = || {
        Err("it names a repository Kule cannot show plainly (only letters, digits, -._~ and single slashes)".to_owned())
    };
    let path = path.strip_prefix('/').unwrap_or(path);
    let path = path.strip_suffix(".git").unwrap_or(path);
    if path.is_empty() || path.len() > MAX_REPO {
        return bad();
    }
    let plain = path.split('/').all(|part| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
    });
    if plain { Ok(path.to_owned()) } else { bad() }
}

/// What an LFS batch request's JSON body asks for: `Ok(true)` for a
/// download, `Ok(false)` for an upload; refused otherwise, and why.
pub(crate) fn lfs_batch(body: &[u8]) -> Result<bool, String> {
    #[derive(serde::Deserialize)]
    struct Batch {
        operation: String,
    }
    // Read as a record: an `operation` given twice is an error, so no
    // reader takes the other one.
    match serde_json::from_slice::<Batch>(body).map(|b| b.operation) {
        Ok(op) if op == "download" => Ok(true),
        Ok(op) if op == "upload" => Ok(false),
        _ => Err("an LFS request Kule cannot read as a download or an upload".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_to(repo: &str) -> Placed {
        Placed::Held(HeldRequest::Push {
            repo: repo.to_owned(),
        })
    }

    #[test]
    fn a_fetch_reads_and_a_push_waits_for_its_repository() {
        let get = |t| place("GET", t, true);
        assert_eq!(
            get("/me/proj.git/info/refs?service=git-upload-pack"),
            Placed::Read
        );
        assert_eq!(
            get("/me/proj/info/refs?service=git-upload-pack"),
            Placed::Read
        );
        // The dumb protocol's discovery reads too.
        assert_eq!(get("/me/proj.git/info/refs"), Placed::Read);
        assert_eq!(
            place("POST", "/me/proj.git/git-upload-pack", false),
            Placed::Read
        );
        // Discovery for a push waits, as the push does, whichever comes
        // first: nothing makes a client ask for refs before it sends.
        assert_eq!(
            get("/me/proj.git/info/refs?service=git-receive-pack"),
            push_to("me/proj")
        );
        assert_eq!(
            place("POST", "/me/proj.git/git-receive-pack", false),
            push_to("me/proj")
        );
        assert_eq!(
            place("POST", "/group/sub/proj/git-receive-pack", false),
            push_to("group/sub/proj")
        );
        assert_eq!(
            place("POST", "/org/project/_git/repo/git-receive-pack", false),
            push_to("org/project/_git/repo")
        );
    }

    #[test]
    fn reads_pass_and_any_other_write_waits() {
        assert_eq!(
            place("GET", "/me/proj/releases/download/v1/x.tar.gz", true),
            Placed::Read
        );
        assert_eq!(place("HEAD", "/", true), Placed::Read);
        assert_eq!(place("OPTIONS", "/api/v4/projects", true), Placed::Read);
        for method in ["POST", "PUT", "PATCH", "DELETE"] {
            assert_eq!(
                place(method, "/api/v4/projects/1/repository/files/x", false),
                Placed::Held(HeldRequest::Write),
                "{method}"
            );
        }
        // A read with a body is not a read.
        assert_eq!(
            place("GET", "/api/v4/projects", false),
            Placed::Held(HeldRequest::Write)
        );
        assert_eq!(
            place("GET", "/me/proj/info/refs?service=git-upload-pack", false),
            Placed::Held(HeldRequest::Write)
        );
        // A fetch to a path Kule cannot show is held as a write.
        assert_eq!(
            place("POST", "/me/%70roj/git-upload-pack", false),
            Placed::Held(HeldRequest::Write)
        );
        assert_eq!(
            place(
                "POST",
                "/me/proj/git-receive-pack/../git-upload-pack",
                false
            ),
            Placed::Held(HeldRequest::Write)
        );
        // Spelled with an escape, a push is not a fetch either.
        assert_eq!(
            place("POST", "/me/proj/git-receive-p%61ck", false),
            Placed::Held(HeldRequest::Write)
        );
    }

    #[test]
    fn a_request_read_two_ways_or_to_a_repository_shown_unplainly_is_refused() {
        let refused = |p: Placed| matches!(p, Placed::Refused(_));
        for target in [
            "/me/proj/info/refs?service=git-receive-pack&service=git-upload-pack",
            "/me/proj/info/refs?service=git-upload-pack&service=git-upload-pack",
            "/me/proj/info/refs?service=git%2Dreceive-pack",
            "/me/proj/info/refs?%73ervice=git-receive-pack",
            "/me/proj/info/refs?service=git-upload-archive",
        ] {
            assert!(refused(place("GET", target, true)), "{target}");
        }
        for target in [
            "/me/../them/proj/git-receive-pack",
            "/me//proj/git-receive-pack",
            "/me/pro%6a/git-receive-pack",
            "/me/pr\u{202e}oj/git-receive-pack",
            "/git-receive-pack",
            "/.git/git-receive-pack",
        ] {
            assert!(refused(place("POST", target, false)), "{target}");
        }
        // Why, as the person reads it in network.jsonl: one service Kule
        // does not know is not two at once.
        let why = |target: &str| match place("GET", target, true) {
            Placed::Refused(why) => why,
            other => panic!("{target}: {other:?}"),
        };
        assert!(
            why("/me/proj/info/refs?service=git-upload-archive").contains("does not know"),
            "{}",
            why("/me/proj/info/refs?service=git-upload-archive")
        );
        assert!(
            why("/me/proj/info/refs?service=git-upload-pack&service=git-upload-pack")
                .contains("two services")
        );
        let long = format!("/{}/git-receive-pack", "a".repeat(MAX_REPO + 1));
        assert!(refused(place("POST", &long, false)));
        let longest = format!("/{}/git-receive-pack", "a".repeat(MAX_REPO));
        assert!(!refused(place("POST", &longest, false)));
    }

    #[test]
    fn lfs_downloads_read_and_its_uploads_and_locks_are_a_push() {
        assert_eq!(
            place("POST", "/me/proj.git/info/lfs/objects/batch", false),
            Placed::LfsBatch {
                repo: "me/proj".to_owned()
            }
        );
        assert_eq!(
            place("GET", "/me/proj.git/info/lfs/locks", true),
            Placed::Read
        );
        for path in ["locks", "locks/verify", "locks/1/unlock", "objects/verify"] {
            assert_eq!(
                place("POST", &format!("/me/proj.git/info/lfs/{path}"), false),
                push_to("me/proj"),
                "{path}"
            );
        }
        // Any LFS request but a bodiless read writes, a read with a body
        // and a bodiless delete too.
        assert_eq!(
            place("GET", "/me/proj.git/info/lfs/locks", false),
            push_to("me/proj")
        );
        assert_eq!(
            place("DELETE", "/me/proj.git/info/lfs/locks/1", true),
            push_to("me/proj")
        );
        assert_eq!(
            lfs_batch(br#"{"operation":"download","objects":[]}"#),
            Ok(true)
        );
        assert_eq!(
            lfs_batch(br#"{"operation": "upload","objects":[]}"#),
            Ok(false)
        );
        for body in [
            &br#"{"operation":"download","operation":"upload"}"#[..],
            br#"{"operation":"delete"}"#,
            br#"{"objects":[]}"#,
            br#"{"operation":1}"#,
            b"not json",
        ] {
            assert!(
                lfs_batch(body).is_err(),
                "{}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[test]
    fn git_hosts_are_known_by_any_spelling() {
        assert!(
            is_git_host("github.com") && is_git_host("GitHub.com.") && is_git_host("gitlab.com")
        );
        assert!(!is_git_host("api.github.com") && !is_git_host("example.com"));
    }
}
