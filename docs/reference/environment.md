---
title: Environment variables
sidebar_label: Environment variables
description: The environment variables Kule reads, and the ones it sets for the commands it runs in a task.
---

# Environment variables

This page lists the variables Kule itself reads, and what the environment of
a command inside a task looks like.

## Variables Kule reads

| Variable | Read by | Meaning |
| --- | --- | --- |
| `KULE_HOME` | `kule`, the daemon, the app | Kule's home. Default `~/.kule`. The same as `--home` on every command. Keep it short: the daemon's socket path inside it must stay under 100 bytes. A home other than `~/.kule` keeps the secrets' key in a file (`secrets.key`) instead of the system keychain. |
| `HOME` | everything | Your home directory: where `~/.kule` is, and what the sandbox hides your own folders under. |
| `PATH` | the app, the sandbox | Where commands and agents (`npx`, `node`, `gemini`, …) are found. Opened from the Finder or a launcher, the app reads `PATH` from your login shell, so it finds the same tools as a terminal. |
| `SHELL` | `kule shell`, the app's terminals | The shell to open in a task's copy. |
| `NO_COLOR` | `kule` | Set (non-empty) to turn colours off. Colour is also off when the output is not a terminal, or `TERM=dumb`. |
| `KULE_TERMINAL` | the app | The terminal app *Open in terminal app* uses: an app name on macOS (for example `iTerm`), a command on Linux. Without it, macOS uses the terminal the app was started from, else Terminal; Linux tries the usual terminals on `PATH`. |
| `KULE_NO_EXTENSIONS` | the app | Set to start the app with every extension off, like `--no-extensions`. |
| `CARGO_HOME` | the sandbox | Where your cargo caches are, to start a task's own from. |
| `XDG_CONFIG_HOME` | the sandbox | When it points at your real `~/.config`, a command gets the home layer's `.config` instead. |
| `XDG_RUNTIME_DIR` | the sandbox (Linux) | Your runtime directory, where session sockets live; kept out of a command's reach. |
| `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GEMINI_API_KEY`, … | the built-in agents | Read only to be **filled into requests** to the agent's own API host; the agent sees a placeholder. See `keys` in [Agents](../using-agents.md). |

## What a command in a task sees

Commands that `kule exec`, `kule run`'s agent, and the app's agents start in
a task get a **trimmed environment**. Only what tools need to find
themselves passes through; tokens, keys and agent sockets in your
environment stay out.

### Passed through as they are

`PATH`, `USER`, `LOGNAME`, `SHELL`, `LANG`, `LANGUAGE`, every `LC_*`, `TZ`,
`TERM`, `COLORTERM`, `NO_COLOR`, `FORCE_COLOR`, `EDITOR`, `PAGER`,
`__CF_USER_TEXT_ENCODING`, `XDG_DATA_HOME`, `XDG_STATE_HOME`, `RUSTUP_HOME`,
`RUSTUP_TOOLCHAIN`, `GOPATH`, `GOROOT`, `JAVA_HOME`, `NVM_DIR`, `NVM_BIN`,
`VOLTA_HOME`, `PYENV_ROOT`, `VIRTUAL_ENV`, `CONDA_PREFIX`, `DOTNET_ROOT`,
`ANDROID_HOME`, `SDKROOT`, `DEVELOPER_DIR`, `HOMEBREW_PREFIX`,
`HOMEBREW_CELLAR`, `HOMEBREW_REPOSITORY`.

To pass one more for a single command: `kule exec --pass-env NAME`. An agent
profile names its own under `pass_env`.

### Set by Kule

| Variable | Value | Why |
| --- | --- | --- |
| `KULE_TASK` | the task's id | Set by `kule exec`, `kule shell` and the app's terminals, so a script can tell which task it runs in. |
| `HOME` | `<task>/home` | The task's [home layer](../concepts/home-layer.md): writes there are caught for review. |
| `XDG_CONFIG_HOME` | `<task>/home/.config` | Only when yours pointed at your real `~/.config`. |
| `TMPDIR` | `<task>/tmp` | The task's own temporary directory. |
| `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` (and lower-case) | `http://127.0.0.1:<port>` | The task's proxy, which judges and logs every connection. `NO_PROXY` is removed. |
| `NODE_USE_ENV_PROXY` | `1` | Makes Node's built-in `fetch` use the proxy. |
| `SSL_CERT_FILE`, `CURL_CA_BUNDLE`, `REQUESTS_CA_BUNDLE`, `PIP_CERT`, `GIT_SSL_CAINFO`, `CARGO_HTTP_CAINFO`, `AWS_CA_BUNDLE` | `<task>/tmp/kule-ca-bundle.pem` | The system's roots plus the task's own certificate authority, which the proxy uses to fill in [secrets](../concepts/secrets.md) for their hosts. |
| `NODE_EXTRA_CA_CERTS` | `<task>/tmp/kule-ca.pem` | The same authority, for Node. |
| `GODEBUG` | adds `x509sslcertoverrideplatform=1` | macOS: makes Go check certificates against `SSL_CERT_FILE`. Your own `GODEBUG` settings are kept. |
| `CARGO_HOME` | `<task>/tmp/caches/cargo` | The task's own cargo cache (with your cargo settings, never `credentials.toml`). |
| `npm_config_cache` | `<task>/tmp/caches/npm` | The task's own npm cache. |
| `PNPM_HOME`, `pnpm_config_store_dir` | under `<task>/tmp/caches/pnpm` | pnpm. |
| `YARN_CACHE_FOLDER`, `PIP_CACHE_DIR`, `UV_CACHE_DIR`, `GOCACHE`, `GOMODCACHE`, `NUGET_PACKAGES`, `XDG_CACHE_HOME` | under `<task>/tmp/caches/` | Each tool's cache, the task's own. |
| `NODE_OPTIONS`, `PYTHONPATH`, `RUBYOPT` | load a small file from `<task>/tmp/.kule-loopback/` | Points Node, Python and Ruby servers that ask for every address at this computer only. (On Linux Kule also holds every program's servers to loopback by itself.) Your own values are kept. |
| a kept secret's name, e.g. `STRIPE_KEY` | `kule_secret_…` | A placeholder; the proxy puts the value in only for that secret's hosts. |

With `kule exec --no-network`, no proxy variables are set and TCP is shut.
With `--open-network`, commands connect directly, without the proxy or the
policy. With `--no-sandbox`, none of the above holds.

On macOS, the cargo and npm caches start as copy-on-write clones of yours, so
they take no space until the task adds to them. On Linux they start empty.

### Look for yourself

```sh
kule exec TASK -- env | sort
```

## See also

- [Files and settings](files.md)
- [Sandbox](../concepts/sandbox.md), [Network and questions](../concepts/network-and-questions.md)
- [`kule exec`](../cli/exec.md)
