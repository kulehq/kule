---
title: Install Kule
sidebar_label: Install
description: Build Kule from source on macOS, Linux or Windows, or download a preview release, then install the agents and check that everything works.
---

# Install Kule

Kule has two programs:

- **`kule`**, the command-line tool;
- **`kule-desktop`**, the desktop app (`Kule.app` on macOS).

Both use the same engine and the same tasks. You can use only the command
line, only the app, or both.

| Platform | Status |
| --- | --- |
| macOS 13 (Ventura) or later, Apple silicon and Intel | Supported |
| Linux x86_64 (Ubuntu 22.04 or later, Fedora, …) | Supported |
| Linux on other architectures | Build from source; no release files |
| Windows 11 24H2 with update 26100.9278 or later, x64 and ARM64 | Supported; see [Platforms](../platforms.md) for what the sandbox needs |
| Windows 10, Windows Server | Not supported: no process container |

:::note A preview

Kule is a **preview**: expect changes from one preview to the next. The
latest pre-release, [`0.2.0-preview.17`](https://github.com/kulehq/kule/releases/tag/v0.2.0-preview.17), is on the releases page:
[download it](#download-a-preview-release), or
[build from source](#build-from-source).

:::

## Build from source

### 1. Install the prerequisites

**Rust.** Install [rustup](https://rustup.rs). You do not need to pick a
version: the repository's
[`rust-toolchain.toml`](https://github.com/kulehq/kule/blob/main/rust-toolchain.toml)
names the exact compiler (Rust 1.99.1), and `cargo` installs it the first
time you build.

**On macOS:** the Xcode command line tools.

```sh
xcode-select --install
```

**On Linux:** a C toolchain, `pkg-config`, and the libraries the desktop app
(Tauri and WebKitGTK) needs. These are the packages Kule's CI installs on
Ubuntu:

```sh
sudo apt-get update
sudo apt-get install -y build-essential pkg-config \
  libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev
```

On Fedora, Tauri's list of equivalents is `webkit2gtk4.1-devel`,
`libappindicator-gtk3-devel`, `librsvg2-devel` and `libxdo-devel`, with a C
compiler and `pkgconf`. If you only want the command line, you can skip the
WebKitGTK libraries and build `-p kule` alone (below).

**On Windows:** the Visual Studio Build Tools with the *Desktop development
with C++* workload (the MSVC compiler and the Windows 11 SDK), and Git. The
app uses WebView2, which Windows 11 has. `rustup` picks the
`-pc-windows-msvc` toolchain by itself.

### 2. Build

```sh
git clone https://github.com/kulehq/kule.git
cd kule
cargo build --release -p kule -p kule-desktop
```

The first build takes a few minutes. When it is done:

| File | What it is |
| --- | --- |
| `target/release/kule` | The command-line tool |
| `target/release/kule-desktop` | The desktop app |

Only the command line, without the app:

```sh
cargo build --release -p kule
```

On Windows the files end in `.exe`, and `kule` needs two more programs
beside it, which it starts for each task: `kule-proxy.exe` (the task's
proxy, in its own app container) and `kule-pty-helper.exe` (the terminal's
pseudo console, inside the task's container). Build and keep the three
together:

```powershell
cargo build --release -p kule -p kule-proxy -p kule-pty-helper
```

### 3. Put `kule` on your `PATH`

Pick a folder that is already on your `PATH`, for example `~/.local/bin`:

```sh
mkdir -p ~/.local/bin
ln -sf "$PWD/target/release/kule" ~/.local/bin/kule
kule --version
```

On Windows, add the folder that holds the three programs to your user
`PATH` (*Settings → System → About → Advanced system settings → Environment
Variables*), or in PowerShell:

```powershell
$user = [Environment]::GetEnvironmentVariable('Path', 'User')
[Environment]::SetEnvironmentVariable('Path', "$user;$PWD\target\release", 'User')
```

Open a new terminal, then `kule --version`.

### 4. Start the app

```sh
./target/release/kule-desktop
```

(`.\target\release\kule-desktop.exe` on Windows.)

During development you can also run `cargo run -p kule-desktop`.

The app starts the Kule **daemon** by itself (it runs its own binary as
`kule-desktop daemon`), so there is nothing else to start. See
[`kule daemon`](../cli/daemon.md) for what the daemon does.

## Download a preview release

Each release carries these files:

| File | For |
| --- | --- |
| `Kule_<version>_universal.dmg` | macOS 13 or later, Apple silicon and Intel in one app. The `kule` command is inside the app. |
| `Kule_<version>_amd64.deb` | Debian and Ubuntu: puts `kule-desktop` and `kule` in `/usr/bin`. |
| `Kule-<version>.x86_64.rpm` | Fedora: the same, as an RPM. |
| `kule-<version>-macos-universal.tar.gz` | The `kule` command alone, for macOS 13 or later. |
| `kule-<version>-linux-x86_64.tar.gz` | The `kule` command alone, for Linux x86_64. |
| `Kule_<version>_x64-setup.exe`, `Kule_<version>_arm64-setup.exe` | Windows 11: installs the app and the `kule` command for you alone, under `%LOCALAPPDATA%\Kule`, and asks for no administrator. |
| `kule-<version>-windows-x64.zip`, `kule-<version>-windows-arm64.zip` | The `kule` command alone, for Windows 11, with the two programs it starts (`kule-proxy.exe`, `kule-pty-helper.exe`), to be kept together. |
| `kule-<version>.cdx.json`, `kule-desktop-<version>.cdx.json` | A CycloneDX SBOM of the CLI and of the app: every crate they are built from. |
| `SHA256SUMS` | The checksum of every file above. |

### Check the download first

Every file has a checksum in `SHA256SUMS` and a **build provenance
attestation**. The attestation proves the file was built from this
repository, by the release workflow, from the release's tag. Check both
before you open anything:

```sh
shasum -a 256 -c SHA256SUMS --ignore-missing
gh attestation verify Kule_<version>_universal.dmg --repo kulehq/kule \
  --signer-workflow kulehq/kule/.github/workflows/release.yml \
  --source-ref refs/tags/v<version>
```

(`gh` is the [GitHub CLI](https://cli.github.com). Replace the file name to
check the `.deb`, `.rpm`, `.tar.gz`, `-setup.exe` or `.zip`. On Windows,
`Get-FileHash` in PowerShell gives a file's SHA-256 to compare with
`SHA256SUMS`.)

### macOS

1. Open the `.dmg` and drag **Kule** to **Applications**.
2. Put the command on your `PATH`:

   ```sh
   ln -sf /Applications/Kule.app/Contents/MacOS/kule ~/.local/bin/kule
   ```

3. Open Kule.

Kule's macOS app is **signed ad hoc, not notarized** by Apple, so macOS
refuses to open it the first time. After checking the download, do one of
these:

- open **System Settings → Privacy & Security** and choose **Open Anyway**
  for Kule; or
- run once:

  ```sh
  xattr -dr com.apple.quarantine /Applications/Kule.app
  ```

The `kule` command from `kule-<version>-macos-universal.tar.gz`, if a
browser downloaded it, needs the same once:
`xattr -d com.apple.quarantine kule`.

### Linux

```sh
# Debian, Ubuntu
sudo apt-get install ./Kule_<version>_amd64.deb
# Fedora
sudo dnf install ./Kule-<version>.x86_64.rpm
```

Both put `kule-desktop` and `kule` in `/usr/bin`. For the command line
alone, unpack the `.tar.gz` and put `kule` on your `PATH`.

:::warning Ubuntu 24.04 and user namespaces

Kule's Linux sandbox needs unprivileged user namespaces, which Ubuntu 24.04
turns off by default. See
[Troubleshooting](../troubleshooting.md) (*Linux: agents do not start*)
before your first agent run.

:::

### Windows

1. Check the download (above). The installer and the programs carry **no
   code signature**, so SmartScreen says the publisher is unknown: choose
   **More info → Run anyway** once you have checked it.
2. Run `Kule_<version>_x64-setup.exe` (or `_arm64-` on an ARM64 computer).
   It installs for you alone, under `%LOCALAPPDATA%\Kule`, and asks for no
   administrator.
3. Put the command on your `PATH`: add `%LOCALAPPDATA%\Kule` to your user
   `PATH` (*Settings → System → About → Advanced system settings →
   Environment Variables*), or in PowerShell:

   ```powershell
   $user = [Environment]::GetEnvironmentVariable('Path', 'User')
   [Environment]::SetEnvironmentVariable('Path', "$user;$env:LOCALAPPDATA\Kule", 'User')
   ```

4. Open Kule from the Start menu.

For the command line alone, unpack `kule-<version>-windows-<arch>.zip` into
a folder of its own and add that folder to your `PATH`. The three programs
in it stay together, and the folder is not Kule's home (`%USERPROFILE%\.kule`):
a task's commands may read the folder of Kule's terminal helper, and Kule's
home they never read, so Kule refuses commands while the two are one.

:::warning Windows 11 24H2 and a desktop session

Kule's Windows sandbox needs Windows 11 24H2 with update 26100.9278 or
later, and a desktop session. Where either is missing, every sandboxed
command is refused with the reason; there is no weaker mode to accept. See
[Platforms](../platforms.md).

:::

## Install the agents

Kule does not ship agents. You install and sign in to each one **the way
you would without Kule**; Kule's runs use that sign-in.

| Agent | What it needs | Sign in |
| --- | --- | --- |
| **Claude Code** | Node.js (Kule starts its ACP adapter with `npx`) | Sign in to Claude Code once in a terminal, or set `ANTHROPIC_API_KEY`. |
| **Codex** | Node.js (`npx`) | `codex login` (a ChatGPT sign-in works), or `OPENAI_API_KEY`. |
| **Gemini** | The Gemini CLI | An API key from Google AI Studio, kept in a file: run `GEMINI_FORCE_FILE_STORAGE=true gemini`, then `/auth`. |

Notes:

- Gemini needs an **AI Studio API key**: Google no longer serves the Code
  Assist sign-in to clients like Kule. The key must be stored in a file,
  not the keychain, because the sandbox keeps the keychain read-only.
- Kule installs each agent adapter that `npx` would fetch **once per
  version**, outside the sandbox and without running the package's install
  scripts, under `~/Library/Application Support/Kule/packages` on macOS
  (`~/.local/share/kule/packages` on Linux and Windows, under your
  profile).
- An API key you give Kule (in *Settings → Secrets*, or as
  `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` / `GEMINI_API_KEY`) is never seen
  by the agent: it gets a placeholder, and the proxy fills in the key. See
  [Secrets](../concepts/secrets.md).
- Other agents that speak the Agent Client Protocol (ACP) can be added. See
  [Agents](../using-agents.md).

## Check that everything works

```sh
kule --version
```

```text
kule 0.2.0-preview.17
```

(The number is the version you built or downloaded.)

Which agents can start, and why the others cannot:

```sh
kule agents list
```

```text
claude-code      built in
gemini           built in
codex            built in

Add your own in ~/.kule/agents.toml (format: kule agents show claude-code).
```

Whether this computer gives the sandbox everything it needs:

```sh
kule isolation
```

```text
this computer gives a sandboxed command all of its isolation
```

On Linux, `kule isolation` lists anything that is missing instead. On
Windows it says the same line, or why sandboxed commands are refused on
this computer. See [`kule isolation`](../cli/isolation.md).

## Where Kule keeps its data

Everything lives in **Kule's home**, `~/.kule` by default
(`%USERPROFILE%\.kule` on Windows): tasks and their copies, settings, the
policy, secrets. Set `KULE_HOME` (or pass `--home`) to use another folder.
No sandbox can see Kule's home. See
[Files and settings](../reference/files.md).

## Next

- [Quick start in a terminal](quick-start.md)
- [Your first task in the app](first-task-in-the-app.md)
