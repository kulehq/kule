#!/bin/sh
# The checks to pass before a release, besides CI: run from the repository
# root, on macOS and on Linux.
#
#   scripts/release-gate.sh [workflows|deps|binary|canary|all]
#   scripts/release-gate.sh attest <tag>     after the release is built
#
# workflows  actionlint and zizmor over .github/workflows
# deps       osv-scanner over the lockfiles (cargo-deny is CI's)
# binary     builds kule with its dependency tree inside (cargo-auditable)
#            and checks that binary with `cargo audit bin`
# canary     plants canary secrets in a workspace's .env files, runs a
#            command in a Kule task, and scans the task's copy and Kule's
#            home for them: there must be none, and every planted value must
#            have its placeholder in the copy. A scan of the workspace itself
#            must find each one first, so an empty result means something.
# attest     verifies every asset of a release against its provenance, held
#            to the release workflow and the tag
#
# Every tool runs at the version pinned below and refuses another; none
# sends what it scans anywhere (betterleaks validates nothing unless told
# to). Not covered here, and still part of the gate: sandbox-probe and a
# real project's install, build and test under `kule exec`.
set -eu

ACTIONLINT=1.7.12
ZIZMOR=1.30.1
OSV_SCANNER=2.6.0
BETTERLEAKS=1.9.0
CARGO_AUDITABLE=0.7.7
CARGO_AUDIT=0.22.2
REPO=kulehq/kule

say() { printf '%s\n' "$*"; }
fail() { say "FAIL: $*" >&2; exit 1; }

# need <tool> <version> <how to install>: the tool, at that version. A Go
# tool built by `go install` says its version only in the build info it
# carries (`mod <path> v<version>`), and cargo-auditable only in the
# source path cargo built it from.
need() {
  command -v "$1" >/dev/null 2>&1 || fail "$1 $2 is not installed: $3"
  bin=$(command -v "$1")
  tab=$(printf '\t')
  "$1" --version 2>/dev/null |
    grep -qE "(^|[^0-9.])v?$(printf '%s' "$2" | sed 's/\./\\./g')([^0-9.]|$)" ||
    grep -aqF "/$1$tab""v$2$tab" "$bin" ||
    grep -aqF "/$1/v${2%%.*}$tab""v$2$tab" "$bin" ||
    grep -aqF "/$1-$2/src/" "$bin" ||
    fail "$1 is not version $2: $3"
}

workflows() {
  need actionlint "$ACTIONLINT" "go install github.com/rhysd/actionlint/cmd/actionlint@v$ACTIONLINT"
  need zizmor "$ZIZMOR" "cargo install --locked zizmor --version $ZIZMOR"
  actionlint
  # Its online audits (impostor commits, stale version comments) ask
  # GitHub's API.
  GH_TOKEN=${GH_TOKEN:-$(gh auth token 2>/dev/null || true)}
  [ -n "$GH_TOKEN" ] || fail "zizmor needs GH_TOKEN (or gh signed in) for its online audits"
  GH_TOKEN=$GH_TOKEN zizmor --no-progress .github/workflows
  say "workflows: ok"
}

deps() {
  need osv-scanner "$OSV_SCANNER" "go install github.com/google/osv-scanner/v2/cmd/osv-scanner@v$OSV_SCANNER"
  # One config for every lockfile, the docs site's (website/) included;
  # what npm installed beside it is not scanned.
  osv-scanner scan source --config osv-scanner.toml --recursive --experimental-exclude target --experimental-exclude node_modules .
  say "deps: ok"
}

binary() {
  need cargo-auditable "$CARGO_AUDITABLE" "cargo install --locked cargo-auditable --version $CARGO_AUDITABLE"
  need cargo-audit "$CARGO_AUDIT" "cargo install --locked cargo-audit --version $CARGO_AUDIT"
  dir=${CARGO_TARGET_DIR:-target}/release-gate
  cargo auditable build --locked --release -p kule --target-dir "$dir"
  # The advisories accepted, with their reasons, in osv-scanner.toml.
  ignored=$(sed -n 's/^id = "\(RUSTSEC-[0-9-]*\)"$/--ignore \1/p' osv-scanner.toml)
  # shellcheck disable=SC2086
  cargo audit --deny warnings $ignored bin "$dir/release/kule"
  say "binary: ok"
}

# A random lower-case hex string of 24 characters.
random_hex() {
  od -An -tx1 -N12 /dev/urandom | tr -d ' \n'
}

# count <report.json>: the findings in a betterleaks JSON report.
count() {
  grep -o '"RuleID"' "$1" | wc -l | tr -d ' '
}

# timed <seconds> <command...>: runs the command, its output as it is. One
# still running after <seconds> is stopped and fails the gate, after what
# it waits on is shown on stderr: the processes, and on macOS a sample of
# each kule process, on fd 3 when it is open (the gate's own stderr, which
# a command's redirect does not reach). A gate never hangs, here or in CI.
timed() {
  perl -e '
    my $to = 3;
    open(my $diag, ">&=", 3) or do { open($diag, ">&", \*STDERR); $to = 2 };
    $diag->autoflush(1);
    my $secs = shift;
    my $pid = fork() // die "fork: $!";
    if ($pid == 0) { exec { $ARGV[0] } @ARGV or die "exec $ARGV[0]: $!" }
    local $SIG{ALRM} = sub {
      print $diag "timed out after ${secs}s: @ARGV\n";
      system("ps -axo pid,ppid,stat,etime,command >&$to");
      if (-x "/usr/bin/sample") {
        for my $p (split /\s+/, `pgrep -f "kule"`) {
          next if $p == $$ || $p !~ /^\d+$/;
          system("/usr/bin/sample $p 2 -mayDie 2>/dev/null | head -80 >&$to");
        }
      }
      kill "TERM", $pid; sleep 2; kill "KILL", $pid;
      exit 124;
    };
    alarm $secs;
    waitpid($pid, 0);
    exit($? >> 8);
  ' "$@"
}

canary() {
  # The gate's stderr, for what timed shows when a command hangs.
  exec 3>&2
  need betterleaks "$BETTERLEAKS" "go install github.com/betterleaks/betterleaks@v$BETTERLEAKS"
  kule=${KULE:-target/debug/kule}
  [ -x "$kule" ] || fail "no kule at $kule: cargo build -p kule, or set KULE"
  kule=$(cd "$(dirname "$kule")" && pwd)/$(basename "$kule")
  work=$(mktemp -d "${TMPDIR:-/tmp}/kule-gate.XXXXXX")
  # KEEP=1 keeps the reports and the task to look at.
  [ -n "${KEEP:-}" ] || trap 'rm -rf "$work"' EXIT INT TERM
  ws=$work/workspace
  home=$work/home
  mkdir -p "$ws/config" "$home"

  # Canaries no real secret can match, and an AWS key pair in AWS's form,
  # for the scanner's own rules. The random bytes are counted out first:
  # where SIGPIPE is ignored (GitHub Actions), macOS's tr reading
  # /dev/urandom whole never stops once head has what it wants.
  aws_id=AKIA$(head -c 4096 /dev/urandom | LC_ALL=C tr -dc 'A-Z2-7' | head -c 16)
  aws_secret=$(head -c 4096 /dev/urandom | LC_ALL=C tr -dc 'A-Za-z0-9/+' | head -c 40)
  [ ${#aws_id} = 20 ] && [ ${#aws_secret} = 40 ] || fail "could not make the AWS canaries"
  cat >"$ws/.env" <<EOF
KULE_CANARY_TOKEN=KULECANARY$(random_hex)
DATABASE_URL=postgres://app:KULECANARY$(random_hex)@db.internal/app
AWS_ACCESS_KEY_ID=$aws_id
AWS_SECRET_ACCESS_KEY=$aws_secret
EOF
  printf 'STRIPE_SECRET=KULECANARY%s\n' "$(random_hex)" >"$ws/config/.env.local"
  planted=5
  # The env files are ignored, as they usually are: the scans must still
  # read them.
  printf '.env*\n' >"$ws/.gitignore"
  printf 'canary workspace\n' >"$ws/README.md"
  git -C "$ws" init -q
  git -C "$ws" add .gitignore README.md
  git -C "$ws" -c user.name=gate -c user.email=gate@localhost commit -qm workspace

  cat >"$work/canary.toml" <<'EOF'
title = "Kule release gate canaries"

[[rules]]
id = "kule-canary"
description = "A canary value the release gate planted"
regex = '''KULECANARY[0-9a-f]{24}'''
keywords = ["kulecanary"]

# A task's base is the workspace as it was, Kule's own to compare with,
# outside every command's reach (checked below).
[allowlist]
paths = ['''/tasks/[0-9a-f]+/base/''']
EOF
  scan() { # <name> <path> [config]: writes $work/<name>.json
    say "canary: scanning $1"
    (cd "$work" && timed 600 betterleaks dir "$2" ${3:+--config "$3"} --no-banner --no-color \
      --redact --report-format json --report-path "$work/$1.json" --exit-code 0 \
      --log-level error)
  }

  # The scans see what was planted.
  scan planted "$ws" "$work/canary.toml"
  [ "$(count "$work/planted.json")" = 3 ] || fail "the canary rule did not find the 3 canaries in the workspace"
  scan planted-aws "$ws"
  grep -q '"RuleID": *"aws' "$work/planted-aws.json" ||
    fail "betterleaks' own rules did not find the AWS key in the workspace"

  # A command in a task reads every env file and its whole environment
  # into the copy.
  export KULE_HOME="$home"
  say "canary: kule new"
  task=$(timed 300 "$kule" new "$ws" | sed -n 's/^task \([0-9a-f]*\) .*/\1/p')
  [ -n "$task" ] || fail "kule new made no task"
  copy=$(timed 60 "$kule" path "$task")
  base=$home/tasks/$task/base
  [ -f "$base/.env" ] || fail "no task base at $base"
  say "canary: kule exec"
  # $1 is the base, expanded by the command's shell.
  # shellcheck disable=SC2016
  timed 300 "$kule" exec "$task" --no-network -- sh -c \
    'env > gate-env.txt; cat .env config/.env.local > gate-seen.txt; cat "$1/.env" >> gate-seen.txt 2>/dev/null && echo "base read" > gate-base.txt; true' \
    gate "$base" >/dev/null 2>&1
  [ -s "$copy/gate-seen.txt" ] || fail "the command in the task wrote nothing"
  [ ! -e "$copy/gate-base.txt" ] || fail "the command read the task's base"

  # Every planted value has its placeholder in the copy.
  placeholders=$(cat "$copy/.env" "$copy/config/.env.local" | grep -c 'kule_secret_' || true)
  [ "$placeholders" = "$planted" ] ||
    fail "$placeholders placeholders in the copy's env files, $planted values planted"

  # And none of the values is anywhere in the task or in Kule's home.
  scan copy "$copy" "$work/canary.toml"
  [ "$(count "$work/copy.json")" = 0 ] || fail "a canary reached the task's copy: $work/copy.json"
  scan home "$home" "$work/canary.toml"
  [ "$(count "$work/home.json")" = 0 ] || fail "a canary is in Kule's home: $work/home.json"
  scan copy-aws "$copy"
  ! grep -q '"RuleID": *"aws' "$work/copy-aws.json" || fail "the AWS key reached the task's copy"
  for value in "$aws_id" "$aws_secret"; do
    ! grep -rqF -D skip --exclude-dir=base -- "$value" "$copy" "$home" ||
      fail "an AWS canary is in the task or Kule's home"
  done
  say "canary: ok ($planted values planted, each masked, none found outside the workspace)"
}

attest() {
  tag=${1:?attest needs the release tag, as v0.2.0}
  command -v gh >/dev/null 2>&1 || fail "gh is not installed"
  dir=$(mktemp -d "${TMPDIR:-/tmp}/kule-attest.XXXXXX")
  trap 'rm -rf "$dir"' EXIT INT TERM
  gh release download "$tag" --repo "$REPO" --dir "$dir"
  for asset in "$dir"/*; do
    gh attestation verify "$asset" --repo "$REPO" \
      --signer-workflow "$REPO/.github/workflows/release.yml" \
      --source-ref "refs/tags/$tag" \
      --deny-self-hosted-runners >/dev/null ||
      fail "$(basename "$asset") has no provenance from the release workflow at $tag"
    say "attested: $(basename "$asset")"
  done
  say "attest: ok"
}

case "${1:-all}" in
  workflows) workflows ;;
  deps) deps ;;
  binary) binary ;;
  canary) canary ;;
  attest) shift; attest "$@" ;;
  all) workflows; deps; binary; canary ;;
  *) fail "unknown check: $1" ;;
esac
