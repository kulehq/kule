//! MCP servers that extensions offer to agents. The agent starts them
//! itself, inside its own sandbox and behind the task's proxy, so a server
//! can do no more than the agent could; what it reaches the policy asks
//! about like anything else.

use std::collections::BTreeMap;
use std::path::Path;

use agent_client_protocol::schema::v1::{EnvVariable, McpServer, McpServerStdio};
use serde::{Deserialize, Serialize};

use crate::packages;

/// A server as the agent is told to start it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpCommand {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

impl McpCommand {
    /// An offered server, `command args…` (`npx` or `uvx` with one exact
    /// version). An npx package is installed once, outside any sandbox and
    /// without its install scripts, and started with node; `home` is the
    /// person's home directory (None: started as written).
    pub fn prepared(
        home: Option<&Path>,
        name: &str,
        command: &str,
        args: &[String],
        env: &BTreeMap<String, String>,
    ) -> McpCommand {
        let mut program = command.to_owned();
        let mut argv = args.to_vec();
        let full: Vec<String> = std::iter::once(command.to_owned())
            .chain(args.iter().cloned())
            .collect();
        if let (Some(home), Some((spec, package, rest))) = (home, packages::npx_package(&full))
            && let Ok(script) =
                packages::installed(&home.join(packages::PACKAGES), &spec, &package, || {})
        {
            program = "node".to_owned();
            argv = std::iter::once(script.to_string_lossy().into_owned())
                .chain(rest)
                .collect();
        }
        McpCommand {
            name: name.to_owned(),
            program,
            args: argv,
            env: env.clone(),
        }
    }

    pub(crate) fn to_acp(&self) -> McpServer {
        McpServer::Stdio(
            McpServerStdio::new(self.name.clone(), self.program.clone())
                .args(self.args.clone())
                .env(
                    self.env
                        .iter()
                        .map(|(k, v)| EnvVariable::new(k.clone(), v.clone()))
                        .collect(),
                ),
        )
    }
}

/// Whether an offered server starts one exact, registry-named package and
/// nothing else: `npx [-y|--yes] name@1.2.3 …` or `uvx name==1.2.3 …`.
/// Options before the package are refused (`--with`, `--from`,
/// `--package`, `--registry` and the like would bring in code the version
/// does not pin), and so is any spec that is a URL, a path or an alias.
/// What follows the package goes to the server itself.
pub fn pinned(command: &str, args: &[String]) -> Result<(), String> {
    match command {
        "npx" => {
            let at = args
                .iter()
                .position(|a| a != "-y" && a != "--yes")
                .ok_or("npx names no package")?;
            let spec = &args[at];
            if spec.starts_with('-') {
                return Err(format!(
                    "npx option {spec} is not taken; the package comes first"
                ));
            }
            npm_spec(spec)
        }
        "uvx" => {
            let spec = args.first().ok_or("uvx names no package")?;
            if spec.starts_with('-') {
                return Err(format!(
                    "uvx option {spec} is not taken; the package comes first"
                ));
            }
            pypi_spec(spec)
        }
        _ => Err(format!("{command}: an MCP server starts with npx or uvx")),
    }
}

/// `name@1.2.3` or `@scope/name@1.2.3`, from the registry.
fn npm_spec(spec: &str) -> Result<(), String> {
    let refuse = || {
        Err(format!(
            "npx {spec}: name a registry package with one exact version, like name@1.2.3"
        ))
    };
    let Some((name, version)) = spec.rsplit_once('@').filter(|(n, _)| !n.is_empty()) else {
        return refuse();
    };
    let plain = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-._~".contains(&b))
    };
    let name_ok = match name.strip_prefix('@') {
        Some(scoped) => scoped
            .split_once('/')
            .is_some_and(|(scope, n)| plain(scope) && plain(n)),
        None => plain(name),
    };
    if !name_ok || !crate::profile::exact_version(version) {
        return refuse();
    }
    Ok(())
}

/// `name==1.2.3`, from the index, without extras.
fn pypi_spec(spec: &str) -> Result<(), String> {
    let ok = spec.split_once("==").is_some_and(|(name, version)| {
        !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
            && crate::profile::exact_version(version)
    });
    if ok {
        Ok(())
    } else {
        Err(format!(
            "uvx {spec}: name an index package with one exact version, like name==1.2.3"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_server_starts_one_exact_registry_package_and_nothing_else() {
        for (command, a) in [
            ("npx", vec!["-y", "@acme/db-mcp@1.2.3", "--stdio"]),
            ("npx", vec!["db-mcp@1.2.3"]),
            ("uvx", vec!["acme-web==2.0.0", "--port", "0"]),
        ] {
            assert!(pinned(command, &args(&a)).is_ok(), "{command} {a:?}");
        }
        for (command, a) in [
            ("npx", vec!["-y", "--package=evil", "tool@1.0.0"]),
            ("npx", vec!["--registry=https://evil.example", "tool@1.0.0"]),
            ("npx", vec!["-y", "tool@latest"]),
            ("npx", vec!["-y", "tool@^1.0.0"]),
            ("npx", vec!["-y", "github:evil/tool"]),
            ("npx", vec!["-y", "tool@npm:evil@1.0.0"]),
            ("npx", vec!["-y", "file:../evil@1.0.0"]),
            ("npx", vec!["-y", "https://evil.example/t.tgz"]),
            ("npx", vec!["-y"]),
            ("uvx", vec!["--with=evil", "tool==1.0.0"]),
            ("uvx", vec!["--from", "git+https://evil", "tool==1.0.0"]),
            ("uvx", vec!["--index-url=https://evil", "tool==1.0.0"]),
            ("uvx", vec!["tool[extra]==1.0.0"]),
            ("uvx", vec!["tool>=1.0.0"]),
            ("uvx", vec!["tool@https://evil"]),
            ("bash", vec!["-c", "x"]),
        ] {
            assert!(pinned(command, &args(&a)).is_err(), "{command} {a:?}");
        }
    }
}
