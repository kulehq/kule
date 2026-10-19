//! `net.fetch` for extensions with the `net` permission: https only, to the
//! hosts the person approved, never to an address on this computer or its
//! network, no redirects, no cookies, with caps on time, size and rate.
//!
//! The address checked is the address connected to: the resolver drops
//! every address that is not public before ureq connects, so a name that
//! resolves to 127.0.0.1 (or changes its answer in between) reaches nothing.

use std::collections::BTreeMap;
use std::io::Read;
use std::time::Duration;

use kule_daemon::public_ip as public;

use serde::{Deserialize, Serialize};
use ureq::http::Uri;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};

/// The largest body sent.
const MAX_SEND: usize = 1024 * 1024;
/// The largest body read back.
const MAX_READ: u64 = 4 * 1024 * 1024;
const MAX_HEADERS: usize = 32;
const TIMEOUT: Duration = Duration::from_secs(15);

/// Headers an extension may not set: they belong to the connection, or
/// would carry the person's identity from elsewhere.
const FORBIDDEN_HEADERS: [&str; 12] = [
    "host",
    "cookie",
    "connection",
    "content-length",
    "transfer-encoding",
    "te",
    "trailer",
    "upgrade",
    "keep-alive",
    "proxy-authorization",
    "proxy-connection",
    "expect",
];

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Init {
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Fetched {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

/// A request made sure of: the method, the host (one of `hosts`) and what
/// goes with it.
#[derive(Debug, PartialEq)]
pub struct Checked {
    pub method: String,
    pub uri: String,
    pub host: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Checks a request against the hosts the person approved.
pub fn check(url: &str, init: Init, hosts: &[String]) -> Result<Checked, String> {
    let uri: Uri = url.parse().map_err(|_| format!("{url:?} is not a URL"))?;
    if uri.scheme_str() != Some("https") {
        return Err("only https:// addresses can be fetched".to_owned());
    }
    let authority = uri.authority().ok_or("the address has no host")?;
    if authority.as_str().contains('@') {
        return Err("an address with a user name or password is not fetched".to_owned());
    }
    if authority.port_u16().is_some_and(|p| p != 443) {
        return Err("only the https port, 443, is reached".to_owned());
    }
    let host = authority.host().to_owned();
    if !hosts.contains(&host) {
        return Err(format!(
            "{host} is not one of the hosts you approved for this extension"
        ));
    }
    let method = init
        .method
        .unwrap_or_else(|| "GET".to_owned())
        .to_ascii_uppercase();
    if !matches!(
        method.as_str(),
        "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE"
    ) {
        return Err(format!("method {method} is not allowed"));
    }
    if init.headers.len() > MAX_HEADERS {
        return Err(format!("at most {MAX_HEADERS} headers"));
    }
    let mut headers = Vec::new();
    for (name, value) in init.headers {
        let lower = name.to_ascii_lowercase();
        let token = !lower.is_empty()
            && lower.len() <= 64
            && lower
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b));
        if !token || FORBIDDEN_HEADERS.contains(&lower.as_str()) || lower.starts_with("proxy-") {
            return Err(format!("header {name:?} cannot be set"));
        }
        if value.len() > 8192 || value.chars().any(|c| c.is_control()) {
            return Err(format!(
                "header {name}'s value is too long or holds control characters"
            ));
        }
        headers.push((lower, value));
    }
    let body = init.body.unwrap_or_default().into_bytes();
    if body.len() > MAX_SEND {
        return Err(format!("a body is at most {} KiB", MAX_SEND / 1024));
    }
    Ok(Checked {
        method,
        uri: uri.to_string(),
        host,
        headers,
        body,
    })
}

/// Sends a checked request.
pub fn send(request: Checked) -> Result<Fetched, String> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .max_redirects(0)
        .https_only(true)
        .http_status_as_error(false)
        .proxy(None)
        .user_agent("Kule extension")
        .build();
    let agent = ureq::Agent::with_parts(
        config,
        DefaultConnector::new(),
        PublicOnly(DefaultResolver::default()),
    );
    let mut builder = ureq::http::Request::builder()
        .method(request.method.as_str())
        .uri(&request.uri);
    for (name, value) in &request.headers {
        builder = builder.header(name, value);
    }
    let built = builder.body(request.body).map_err(|e| e.to_string())?;
    let mut response = agent.run(built).map_err(|e| match e {
        ureq::Error::HostNotFound => format!(
            "{} has no public address (Kule does not reach this computer or its network)",
            request.host
        ),
        e => e.to_string(),
    })?;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .filter(|(name, _)| name.as_str() != "set-cookie")
        .filter_map(|(name, value)| Some((name.to_string(), value.to_str().ok()?.to_owned())))
        .collect();
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_READ + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_READ {
        return Err(format!("the answer is over {} MiB", MAX_READ / 1024 / 1024));
    }
    Ok(Fetched {
        status,
        headers,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

/// A resolver that keeps public addresses only.
#[derive(Debug)]
struct PublicOnly<R>(R);

impl<R: Resolver> Resolver for PublicOnly<R> {
    fn resolve(
        &self,
        uri: &Uri,
        config: &ureq::config::Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let all = self.0.resolve(uri, config, timeout)?;
        let mut kept = self.empty();
        kept.truncate(0);
        for addr in all.iter().filter(|a| public(a.ip())) {
            kept.push(*addr);
        }
        if kept.is_empty() {
            return Err(ureq::Error::HostNotFound);
        }
        Ok(kept)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosts() -> Vec<String> {
        vec!["hooks.slack.com".to_owned()]
    }

    #[test]
    fn only_https_to_an_approved_host_on_443_is_sent() {
        let ok = check(
            "https://hooks.slack.com/services/T/B/x",
            Init::default(),
            &hosts(),
        )
        .unwrap();
        assert_eq!(
            (ok.method.as_str(), ok.host.as_str()),
            ("GET", "hooks.slack.com")
        );
        assert!(check("https://hooks.slack.com:443/x", Init::default(), &hosts()).is_ok());
        for bad in [
            "http://hooks.slack.com/x",
            "https://evil.example/x",
            "https://hooks.slack.com.evil.example/x",
            "https://hooks.slack.com:8443/x",
            "https://user:pw@hooks.slack.com/x",
            "https://127.0.0.1/x",
            "file:///etc/passwd",
            "//hooks.slack.com/x",
            "not a url",
        ] {
            assert!(check(bad, Init::default(), &hosts()).is_err(), "{bad}");
        }
    }

    #[test]
    fn methods_headers_and_bodies_are_held_to_what_is_safe() {
        let with = |method: &str, header: (&str, &str), body: &str| Init {
            method: Some(method.to_owned()),
            headers: [(header.0.to_owned(), header.1.to_owned())].into(),
            body: Some(body.to_owned()),
        };
        let url = "https://hooks.slack.com/x";
        assert!(
            check(
                url,
                with("post", ("Content-Type", "application/json"), "{}"),
                &hosts()
            )
            .is_ok()
        );
        assert!(
            check(
                url,
                with("POST", ("Authorization", "Bearer x"), ""),
                &hosts()
            )
            .is_ok()
        );
        assert!(check(url, with("CONNECT", ("a", "b"), ""), &hosts()).is_err());
        for header in [
            "Host",
            "Cookie",
            "Proxy-Authorization",
            "Transfer-Encoding",
            "X Bad",
            "",
        ] {
            assert!(
                check(url, with("GET", (header, "x"), ""), &hosts()).is_err(),
                "{header}"
            );
        }
        assert!(check(url, with("GET", ("X-A", "a\r\nHost: evil"), ""), &hosts()).is_err());
        let big = "x".repeat(MAX_SEND + 1);
        assert!(check(url, with("POST", ("X-A", "a"), &big), &hosts()).is_err());
    }

    /// A name that resolves to this computer reaches nothing.
    #[derive(Debug)]
    struct Local;
    impl Resolver for Local {
        fn resolve(
            &self,
            _: &Uri,
            _: &ureq::config::Config,
            _: NextTimeout,
        ) -> Result<ResolvedSocketAddrs, ureq::Error> {
            let mut all = self.empty();
            all.truncate(0);
            all.push("127.0.0.1:443".parse().unwrap());
            all.push("[::1]:443".parse().unwrap());
            Ok(all)
        }
    }

    #[test]
    fn a_name_that_resolves_here_is_refused_before_connecting() {
        let config = ureq::Agent::config_builder().build();
        let agent = ureq::Agent::with_parts(config, DefaultConnector::new(), PublicOnly(Local));
        let e = agent.get("https://hooks.slack.com/x").call().unwrap_err();
        assert!(matches!(e, ureq::Error::HostNotFound), "{e:?}");
    }
}
