# Maintaining Kule

What the maintainer does between commits: before a release, at a release,
when an operating system ships a new major version, and when the weekly CI
run fails. Written for one person today; it holds for more.

## Before a release

CI runs on every pull request: `cargo fmt`, clippy with warnings denied,
the whole test suite on the real filesystem on macOS, Linux and Windows,
the release gate's `canary`, `probe` and `project` steps on macOS and
Linux (`scripts/release-gate.sh`), and the Windows sandbox tests on a
Windows 11 runner, required to run and not skip. Besides that:

- **Windows, as a standard user.** The hosted Windows 11 runner's account
  is an administrator. Before a tag, run the container tests on a real
  Windows 11 24H2 machine as a standard user, with
  `KULE_TEST_REQUIRE_SANDBOX=1`: `sandbox_windows` and `terminal_windows`
  in `kule-engine`, `container` in `kule-proxy`, then `kule exec` end to
  end (a write in the copy lands, a write outside is refused and leaves
  nothing). Record the result in the release pull request (issue #64).
- **Clippy on every platform.** A `cfg` branch that only one platform
  compiles is only checked there.
- **The docs site builds** in both locales (`cd website && npm run build`),
  with broken links set to throw.

## Releasing

1. One pull request carries the batch: the changes, the CHANGELOG section
   under the new version, and the bump of `Cargo.toml`, every crate's
   inter-workspace versions, `Cargo.lock`, `crates/kule-desktop/tauri.conf.json`
   and the issue forms' version placeholders. The install docs' version
   links stay at the previous release until the new one exists.
2. After the merge, wait for `ci.yml` to be green on the merge commit
   (`gh run list --branch main --workflow ci.yml`). A red main is never
   tagged.
3. Tag the merge commit: `git tag -a v<version> -m "Kule <version>"`, then
   `git push origin v<version>`. `release.yml` builds every platform, runs
   the gate on the very binaries it will publish (`gate`, `gate-windows`),
   and waits for the `release` environment's approval before it makes a
   **draft** release.
4. Check the draft: the assets (`.dmg`, `.deb`, `.rpm`, two `.tar.gz`, two
   `-setup.exe`, two `.zip`, two `.cdx.json`, `SHA256SUMS`), and
   `scripts/release-gate.sh attest v<version>`, which verifies every asset's
   provenance against the workflow and the tag. Then publish it.
5. A second pull request points the install docs at the release:
   `docs/index.md`, `docs/getting-started/install.md`, their Turkish
   translations, and the README's status line.
6. A published advisory whose fix shipped in this release gets its
   `patched_versions` set to it.

## When an operating system ships a new major version

The sandbox rests on what each operating system provides: Seatbelt on
macOS, Landlock, seccomp and namespaces on Linux, the process container on
Windows. A new macOS, a new kernel or a new Windows build can change what
those hide, refuse or allow without a commit here. For each new major
version of macOS, Ubuntu LTS (as GitHub's runner images follow it) and
Windows 11 feature update:

1. **Add the new runner image** to the `test` matrix in `ci.yml` beside
   the `-latest` labels (and `windows-11-arm`'s successor for the sandbox
   job, if there is one), as the non-required entry. When the `-latest`
   label moves to it, the previous entry goes. The labels are listed in
   [actions/runner-images](https://github.com/actions/runner-images#available-images).
2. **Review what Kule hides and refuses** against what the release adds:
   the lists in `crates/kule-engine/src/sandbox.rs` (`HIDDEN_IN_HOME`,
   `PERSONAL_IN_HOME`, `SETTINGS_IN_HOME`, `SETTINGS_READ`, `OUTSIDE_SERVICES`,
   `NEVER_DEVICES`, `PASSED_ENV` and `PASSED_ENV_WINDOWS`) and the
   platform's own module (`crates/kule-engine/src/sandbox/windows.rs`,
   `crates/kule-engine/src/windows/`). A new credential store, a new
   folder of the person's own, a new service a program reaches, a new
   environment variable tools read: each is hidden or refused by default,
   and gets a test that fails without it.
3. **Run the gate on the new release**: `scripts/release-gate.sh canary`,
   `probe` and `project` on macOS and Linux, and the Windows container
   tests on the new build, as a standard user. On Windows, check
   `LEAST_BUILD` in `crates/kule-engine/src/windows/preconditions.rs` still
   names the oldest build the container holds on, and `docs/windows-measurements.md`
   still describes what was measured.
4. **Note the result** in the CHANGELOG entry of the next release and in
   `docs/platforms.md` (English and Turkish), including what the new
   release changed for Kule, if anything.

## When the weekly run fails

`ci.yml` runs every Monday on `main` with nothing changed (`schedule:`).
When it fails, its `report` job opens an issue labelled `scheduled-ci`, or
comments on the one already open. Look at, in this order:

- a runner image that moved to a new OS release (the `-latest` labels, and
  the non-required next-OS entry): the checklist above;
- a dependency or tool the gate installs at a pinned version that no
  longer builds or runs on the image (Go, Node, `sandbox-probe`,
  `betterleaks`, `osv-scanner`, `cargo-deny`, `actionlint`, `zizmor`);
- an advisory in `cargo deny` or `osv-scanner` against a dependency:
  update it, or record the decision in `deny.toml`.

The next-OS entry is not a required check: a failure there blocks no
pull request, and is fixed through the checklist above rather than by
removing the entry.
