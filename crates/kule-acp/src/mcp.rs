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

/// Whether an offered server names its package with one exact version:
/// `npx [-y] name@1.2.3 …` or `uvx name==1.2.3 …`.
pub fn pinned(command: &str, args: &[String]) -> Result<(), String> {
    let package = args.iter().find(|a| !a.starts_with('-'));
    if package.is_none() {
        return Err(format!("{command} names no package"));
    }
    crate::profile::check_pinned(command, args)
}
