#!/bin/sh
# The checks to pass before a release, besides CI: run from the repository
# root, on macOS and on Linux.
#
# Where each runs on Linux: workflows and deps read the same files on
# every system, and CI runs them on Linux for every change (its workflows
# and deny jobs). canary, probe and project run in CI on macOS and Linux;
# probe, project and binary also run in the release workflow, on the very
# binaries it is about to publish. attest runs anywhere, after the
# release.
#
#   scripts/release-gate.sh [workflows|deps|binary|canary|probe|project|all]
#   scripts/release-gate.sh attest <tag>     after the release is built
#
# workflows  actionlint and zizmor over .github/workflows
# deps       osv-scanner over the lockfiles (cargo-deny is CI's)
# binary     checks the crates inside kule against the advisories with
#            `cargo audit bin`: the binary KULE names (each slice of a macOS
#            universal one), or one built here; either must carry its
#            dependency tree (cargo-auditable), as the release builds do
# canary     plants canary secrets in a workspace's .env files, runs a
#            command in a Kule task, and scans the task's copy and Kule's
#            home for them: there must be none, and every planted value must
#            have its placeholder in the copy. A scan of the workspace itself
#            must find each one first, so an empty result means something.
# probe      runs sandbox-probe in a Kule task, through the proxy and with
#            no network, against a home holding something at every path
#            Kule hides: none may be read there, nor Kule's own key, the
#            task's base or a project beside the workspace; the workspace
#            may not be written, the network not reached, no secret in the
#            environment seen. The same scan outside the task must find
#            each of them first.
# project    installs, builds and tests real projects in a Kule task, from
#            the registries the policy lets through (ripgrep from
#            crates.io, commander from npm), with a home that holds tokens
#            and settings; the workspace stays as it was, the token is
#            masked, and a host the policy does not name is refused.
# attest     verifies every asset of a release against its provenance, held
#            to the release workflow and the tag
#
# Every tool runs at the version pinned below and refuses another; none
# sends a file it scans anywhere (betterleaks validates nothing unless
# told to; sandbox-probe asks google.com whether it can get out;
# osv-scanner asks osv.dev about the package names and versions in the
# lockfiles).
set -eu

ACTIONLINT=1.7.12
ZIZMOR=1.30.1
OSV_SCANNER=2.6.0
BETTERLEAKS=1.9.0
CARGO_AUDITABLE=0.7.7
CARGO_AUDIT=0.22.2
SANDBOX_PROBE=6.19.1
# The real projects, each at a commit: ripgrep 15.2.0 and commander
# 15.0.0. Their lockfiles name nothing but crates.io and npm.
RIPGREP=e89fff89ac9af12e8d4ce9d5fd07beb408ca730f
COMMANDER=ba6d13ddb4243e5913367734f8c159089ffe7834
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

# audit <binary>: the crates inside it against the advisories. It must
# carry its dependency tree (cargo-auditable): of any other binary, cargo
# audit reads the few crates it can find, passes it, and only warns.
audit() {
  # The advisories accepted, with their reasons, in osv-scanner.toml.
  ignored=$(sed -n 's/^id = "\(RUSTSEC-[0-9-]*\)"$/--ignore \1/p' osv-scanner.toml)
  # shellcheck disable=SC2086
  out=$(cargo audit --deny warnings $ignored bin "$1" 2>&1) || {
    say "$out"
    fail "$1 has crates with advisories"
  }
  say "$out"
  printf '%s\n' "$out" | grep -q "Found 'cargo auditable' data in" ||
    fail "$1 carries no dependency tree: build it with cargo auditable"
}

binary() {
  need cargo-audit "$CARGO_AUDIT" "cargo install --locked cargo-audit --version $CARGO_AUDIT"
  if [ -n "${KULE:-}" ]; then
    # The binary a release built. A macOS universal one is audited slice
    # by slice: cargo audit reads no dependency tree from it whole.
    find_kule
    archs=$(lipo -archs "$kule" 2>/dev/null || true)
    case "$archs" in
    *" "*)
      scratch
      for arch in $archs; do
        lipo -thin "$arch" -output "$work/kule-$arch" "$kule"
        audit "$work/kule-$arch"
      done
      ;;
    *) audit "$kule" ;;
    esac
  else
    need cargo-auditable "$CARGO_AUDITABLE" "cargo install --locked cargo-auditable --version $CARGO_AUDITABLE"
    dir=${CARGO_TARGET_DIR:-target}/release-gate
    cargo auditable build --locked --release -p kule --target-dir "$dir"
    audit "$dir/release/kule"
  fi
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

# find_kule: $kule, the binary under test, by its full path.
find_kule() {
  kule=${KULE:-target/debug/kule}
  [ -x "$kule" ] || fail "no kule at $kule: cargo build -p kule, or set KULE"
  kule=$(cd "$(dirname "$kule")" && pwd)/$(basename "$kule")
}

# scratch [dir]: $work, a new directory in <dir> (TMPDIR by default),
# removed when the gate ends. KEEP=1 keeps it, with the reports and the
# tasks, to look at.
scratched=
scratch() {
  work=$(mktemp -d "${1:-${TMPDIR:-/tmp}}/kule-gate.XXXXXX")
  scratched="$scratched $work"
  # shellcheck disable=SC2086
  [ -n "${KEEP:-}" ] || trap 'rm -rf $scratched' EXIT INT TERM
}

canary() {
  # The gate's stderr, for what timed shows when a command hangs.
  exec 3>&2
  need betterleaks "$BETTERLEAKS" "go install github.com/betterleaks/betterleaks@v$BETTERLEAKS"
  find_kule
  scratch
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

# hidden_paths: what Kule hides from every command under the home
# directory (HIDDEN_IN_HOME and PERSONAL_IN_HOME), relative, one a line,
# read from the source: a path added there is checked here with it.
hidden_paths() {
  sed -n '/^pub const HIDDEN_IN_HOME/,/^];/p;/^pub const PERSONAL_IN_HOME/,/^];/p' \
    crates/kule-engine/src/sandbox.rs | sed -n 's/^ *"\(.*\)",$/\1/p'
}

# is_file <path>: whether what stands at <path> is a file (`.netrc`,
# `.m2/settings.xml`) rather than a folder (`.ssh`), by its name.
is_file() {
  b=${1##*/}
  b=${b#.}
  case $b in
    *.* | *history | netrc | npmrc | pypirc | git-credentials | vault-token | pgpass | \
      lesshst | viminfo | databrickscfg | token | credentials | hub) return 0 ;;
  esac
  return 1
}

# block <path> <label> [file]: an entry of sandbox-probe's expectations
# in $config: nothing at <path> may be read or written in the task, nor
# <file> in the folder at <path>, which goes to $folders for the writes
# below. What the scan outside the task must find first goes to
# $expected, as `open <label>`.
block() {
  {
    printf '    - path: "%s"\n      label: "%s"\n      severity: critical\n' "$1" "$2"
    printf '      reason: kept from every command\n'
    if [ -n "${3:-}" ]; then
      printf '      check_files: [%s]\n' "$3"
      [ -z "$folder_ops" ] || printf '      check_ops: [%s]\n' "$folder_ops"
    fi
  } >>"$config"
  if [ -n "${3:-}" ]; then
    printf 'open %s/%s\n' "$2" "$3"
    printf '%s\n' "$1" >>"$folders"
  else
    printf 'open %s\n' "$2"
  fi >>"$expected"
}

# writes <folders> <out>: each of the folders a file could be made in,
# one a line, to <out>. Run in a task and outside it.
# shellcheck disable=SC2016
WRITES='while IFS= read -r d; do
  if (: >"$d/kule-gate-write") 2>/dev/null; then printf "%s\n" "$d"; fi
done <"$1" >"$2"'


# finding <report> <type>: the values of the findings of <type>, as JSON.
finding() {
  jq -c --arg t "$2" '[.findings[] | select(.findingType == $t) | .value]' "$1"
}

probe() {
  exec 3>&2
  need sandbox-probe "$SANDBOX_PROBE" "go install github.com/controlplaneio/sandbox-probe/v6@v$SANDBOX_PROBE"
  command -v jq >/dev/null 2>&1 || fail "jq is not installed"
  find_kule
  # Not in /tmp: commands may write there, and on Linux a task may have a
  # /tmp of its own, which would hide what is planted for the wrong reason.
  scratch "$HOME"
  case $(cd "$work" && pwd -P) in
    /tmp/* | /private/tmp/*) fail "the probe's scratch is in /tmp: $work" ;;
  esac
  home=$work/home
  ws=$home/code/app
  mkdir -p "$ws" "$home/code/beside" "$home/.config/tool"

  # Something at every path Kule hides, each with something to read.
  hidden_paths >"$work/hidden.txt"
  held=$(wc -l <"$work/hidden.txt" | tr -d ' ')
  [ "$held" -ge 50 ] || fail "read $held hidden paths from crates/kule-engine/src/sandbox.rs: its lists moved"
  # These stay on the list, whatever else changes on it.
  for rel in .ssh .aws .gnupg .kube .docker .netrc .config/gh Library/Keychains Documents; do
    grep -qxF "$rel" "$work/hidden.txt" || fail "Kule no longer hides ~/$rel"
  done
  while IFS= read -r rel; do
    if is_file "$rel"; then
      mkdir -p "$home/$(dirname "$rel")"
      printf 'decoy\n' >"$home/$rel"
    else
      mkdir -p "$home/$rel"
      printf 'decoy\n' >"$home/$rel/decoy"
    fi
  done <"$work/hidden.txt"
  # A token in the home's own .env: Kule masks it in the task's home and
  # hides the real file. A tool's settings Kule does not list are hidden
  # whole.
  printf 'API_TOKEN=KULECANARY%s\n' "$(random_hex)" >"$home/.env"
  printf 'API_TOKEN=KULECANARY%s\n' "$(random_hex)" >"$home/.config/tool/.env"
  printf 'decoy\n' >"$home/code/beside/decoy"
  printf 'probe workspace\n' >"$ws/README.md"
  cp "$(command -v sandbox-probe)" "$ws/sandbox-probe"
  git -C "$ws" init -q
  git -C "$ws" add README.md
  git -C "$ws" -c user.name=gate -c user.email=gate@localhost commit -qm workspace

  # Secrets in the environment the task starts from: a GitHub token and
  # an AWS key, in their forms.
  GITHUB_TOKEN=ghp_$(head -c 4096 /dev/urandom | LC_ALL=C tr -dc 'A-Za-z0-9' | head -c 36)
  AWS_SECRET_ACCESS_KEY=$(head -c 4096 /dev/urandom | LC_ALL=C tr -dc 'A-Za-z0-9/+' | head -c 40)
  export GITHUB_TOKEN AWS_SECRET_ACCESS_KEY
  export HOME="$home" KULE_HOME="$work/kule"
  say "probe: kule new"
  task=$(timed 300 "$kule" new "$ws" | sed -n 's/^task \([0-9a-f]*\) .*/\1/p')
  [ -n "$task" ] || fail "kule new made no task"
  copy=$(timed 60 "$kule" path "$task")
  # A first command makes the task's home and seals the token there.
  timed 300 "$kule" exec "$task" --no-network -- true >/dev/null 2>&1 || fail "a command did not run in the task"
  for f in "$KULE_HOME/secrets.key" "$KULE_HOME/tasks/$task/secrets.json"; do
    [ -f "$f" ] || fail "nothing sealed the token: no $f"
  done

  config=$copy/probe.yaml
  expected=$work/expected.txt
  folders=$copy/folders.txt
  : >"$expected"
  : >"$folders"
  # Whether a folder may be written, sandbox-probe asks access(2), which
  # Landlock does not answer: on Linux the writes below say it.
  case $(uname) in
    Darwin) folder_ops= ;;
    *) folder_ops='readdir, open' ;;
  esac
  printf 'custom_paths:\n  must_block:\n' >"$config"
  while IFS= read -r rel; do
    if is_file "$rel"; then block "$home/$rel" "$rel"; else block "$home/$rel" "$rel" decoy; fi
  done <"$work/hidden.txt"
  block "$home/.env" "masked ~/.env"
  block "$home/.config/tool/.env" "an unlisted tool's settings"
  block "$home/code/beside" "a project beside the workspace" decoy
  block "$KULE_HOME/tasks/$task/base" "the task's base" README.md
  block "$KULE_HOME/tasks/$task/secrets.json" "the task's sealed values"
  block "$KULE_HOME/secrets.key" "the key that seals them"
  # Folders a command may not make a file in, though they are no secret:
  # the workspace, the home (the task writes in its own layer), Kule's
  # home and the task's own folder (beside its copy and tmp), and what is
  # beside the home.
  for dir in "$ws" "$home" "$KULE_HOME" "$KULE_HOME/tasks/$task" "$work"; do
    printf '%s\n' "$dir" >>"$folders"
  done
  cat >>"$config" <<EOF
  must_readwrite:
    - path: "$copy"
      label: the task's copy
      severity: critical
      reason: the command works there
EOF

  # Outside the task, each is there to find.
  say "probe: scanning outside the task"
  (cd "$work" && timed 600 "$copy/sandbox-probe" scan --config "$config" --output_path "$work/outside.json") \
    >"$work/outside.log" 2>&1 || true
  [ -s "$work/outside.json" ] || fail "the scan outside the task wrote no report: $work/outside.log"
  jq -r '.findings[] | select(.findingType == "custom_path_violation") | .value | "\(.op) \(.label)"' \
    "$work/outside.json" | sort -u >"$work/outside.seen"
  missing=$(grep -vxFf "$work/outside.seen" "$expected" || true)
  [ -z "$missing" ] || fail "outside the task, the scan could not reach: $missing"
  finding "$work/outside.json" env_secret_detection | grep -q '"GITHUB_TOKEN"' ||
    fail "outside the task, the scan did not see GITHUB_TOKEN"
  [ "$(finding "$work/outside.json" external_host_connectivity | jq 'flatten | length')" -gt 0 ] ||
    fail "outside the task, the scan reached no host: an empty result in it would mean nothing"

  for net in proxy none; do
    flag=
    [ "$net" = proxy ] || flag=--no-network
    say "probe: scanning in the task (network: $net)"
    status=0
    # shellcheck disable=SC2086
    timed 600 "$kule" exec "$task" $flag -- ./sandbox-probe scan --config probe.yaml \
      --output_path "in-$net.json" >"$work/in-$net.log" 2>&1 || status=$?
    report=$copy/in-$net.json
    [ -s "$report" ] || fail "the scan in the task wrote no report: $work/in-$net.log"
    cp "$report" "$work/"
    reached=$(jq -r '.findings[] | select(.findingType == "custom_path_violation") | .value.message' "$report")
    [ -z "$reached" ] || fail "in the task (network: $net): $reached"
    [ "$status" = 0 ] || fail "sandbox-probe failed in the task (network: $net): $work/in-$net.log"
    for t in external_host_connectivity external_host_dns_resolution; do
      [ "$(finding "$report" "$t")" = '[[]]' ] || fail "in the task (network: $net), $t: $(finding "$report" "$t")"
    done
    [ "$(finding "$report" env_secret_detection)" = '[]' ] ||
      fail "in the task, secrets in the environment: $(finding "$report" env_secret_detection)"
    # Seatbelt on macOS; on Linux the scan reads Landlock off a line
    # /proc/self/status does not have (see the layers below), and finds
    # the task's own user namespace.
    case $(uname) in
      Darwin) want=seatbelt ;;
      *) want=user-namespace ;;
    esac
    finding "$report" sandbox_detection | grep -q "\"$want\"" ||
      fail "in the task, no $want: $(finding "$report" sandbox_detection)"
    if [ "$net" = proxy ]; then
      finding "$report" proxy_detection | grep -q '"https_proxy":"http://127.0.0.1:' ||
        fail "in the task, the network does not go through Kule's proxy"
    fi
  done

  # On Linux, the command is under Landlock: sandbox-probe stacks layers
  # until the kernel's 16 are reached, all of them outside the task, fewer
  # in it.
  if [ "$(uname)" = Linux ]; then
    outside=$("$copy/sandbox-probe" probe | jq .lockdownDepth)
    [ "$outside" = 16 ] || fail "outside the task, Landlock's depth is $outside, not 16"
    timed 120 "$kule" exec "$task" --no-network -- sh -c './sandbox-probe probe > layers.json' \
      >"$work/layers.log" 2>&1 || fail "the layers were not counted in the task: $work/layers.log"
    inside=$(jq .lockdownDepth "$copy/layers.json")
    [ "$inside" -lt 16 ] || fail "in the task, no Landlock: $inside layers left of 16"
  fi

  # A file made in each folder: none in the task, every one outside it.
  printf '%s\n' "$WRITES" >"$copy/writes.sh"
  say "probe: writing in the task"
  timed 300 "$kule" exec "$task" -- sh writes.sh folders.txt wrote.txt >"$work/writes.log" 2>&1 ||
    fail "the writes did not run in the task: $work/writes.log"
  [ -f "$copy/wrote.txt" ] || fail "the writes in the task said nothing: $work/writes.log"
  [ ! -s "$copy/wrote.txt" ] || fail "in the task, a file was made in: $(cat "$copy/wrote.txt")"
  sh "$copy/writes.sh" "$folders" "$work/wrote-outside.txt"
  while IFS= read -r dir; do rm -f "$dir/kule-gate-write"; done <"$folders"
  missing=$(grep -vxFf "$work/wrote-outside.txt" "$folders" || true)
  [ -z "$missing" ] || fail "outside the task, no file could be made in: $missing"
  say "probe: ok ($(wc -l <"$expected" | tr -d ' ') paths and $(wc -l <"$folders" | tr -d ' ') folders found outside the task, none reached in it; no host, no secret in the environment)"
}

# fetch <name> <url> <commit>: the project at that commit, as a workspace
# in the gate's home.
fetch() {
  dir=$HOME/code/$1
  mkdir -p "$dir"
  git -C "$dir" init -q
  timed 300 git -C "$dir" fetch -q --depth 1 "$2" "$3"
  git -C "$dir" -c advice.detachedHead=false checkout -q FETCH_HEAD
  [ "$(git -C "$dir" rev-parse HEAD)" = "$3" ] || fail "$1 is not at $3"
}

# build <name> <script>: runs the script in a task on the project, its
# network through the task's proxy, its output in $work/<name>.log. The
# workspace stays as it was, and nothing gets out but to the registries.
build() {
  dir=$HOME/code/$1
  task=$(timed 300 "$kule" new "$dir" | sed -n 's/^task \([0-9a-f]*\) .*/\1/p')
  [ -n "$task" ] || fail "kule new made no task for $1"
  say "project: $1, in task $task"
  timed 2400 "$kule" exec "$task" -- sh -c "$2" >"$work/$1.log" 2>&1 || {
    tail -40 "$work/$1.log" >&2
    fail "$1 did not install, build and test in the task: $work/$1.log"
  }
  [ -z "$(git -C "$dir" status --porcelain --ignored)" ] || fail "the task changed $1's workspace"
  out=$(grep '^net: ' "$work/$1.log" | grep -v '(package-registries)$' | grep -v ' refused' || true)
  [ -z "$out" ] || fail "$1 reached a host the policy does not name: $out"
}

project() {
  exec 3>&2
  find_kule
  for tool in git cargo node npm curl; do
    command -v "$tool" >/dev/null 2>&1 || fail "$tool is not installed"
  done
  node -e 'const [a, b] = process.versions.node.split(".").map(Number); process.exit(a > 22 || (a === 22 && b >= 12) ? 0 : 1)' ||
    fail "commander $COMMANDER needs Node 22.12 or newer"
  scratch
  home=$work/home
  mkdir -p "$home/.config/tool" "$home/code"
  # A home as people have it: a .env with a token, switches and a number
  # whose keys look like secrets, and a startup file.
  token=KULECANARY$(random_hex)
  cat >"$home/.env" <<EOF
API_TOKEN=$token
TOOL_ALLOW_PRIVATE_NETWORKS=false
TOOL_AUTH_ENABLED=true
MAX_TOKENS=4096
EOF
  printf 'export TOOL_PRIVATE_MODE=false\n' >"$home/.zshrc"
  # The toolchains stay where they are; the home is the gate's.
  if [ -z "${RUSTUP_HOME:-}" ] && [ -d "$HOME/.rustup" ]; then
    export RUSTUP_HOME="$HOME/.rustup"
  fi
  export HOME="$home" KULE_HOME="$work/kule"
  fetch ripgrep https://github.com/BurntSushi/ripgrep.git "$RIPGREP"
  fetch commander https://github.com/tj/commander.js.git "$COMMANDER"

  # Every crate comes from crates.io in the task: its cargo home is new.
  # shellcheck disable=SC2016
  build ripgrep 'export CARGO_HOME="$TMPDIR/cargo-home"
    cargo fetch --locked && cargo build --locked --offline && cargo test --locked --offline --workspace'
  ! grep -q '^test result: FAILED' "$work/ripgrep.log" || fail "ripgrep's tests failed: $work/ripgrep.log"
  passed=$(sed -n 's/^test result: ok\. \([0-9]*\) passed.*/\1/p' "$work/ripgrep.log" | awk '{ s += $1 } END { print s + 0 }')
  [ "$passed" -ge 400 ] || fail "ripgrep: $passed tests passed, 400 or more run: $work/ripgrep.log"
  fetched=$(grep -c '^ *Downloaded ' "$work/ripgrep.log" || true)
  [ "$fetched" -ge 30 ] || fail "ripgrep: $fetched crates downloaded in the task, 30 or more expected"
  say "project: ripgrep ok ($fetched crates from crates.io, $passed tests passed)"

  # shellcheck disable=SC2016
  build commander 'export npm_config_cache="$TMPDIR/npm-cache"
    npm ci --no-audit --no-fund && npm test'
  added=$(sed -n 's/^added \([0-9]*\) packages.*/\1/p' "$work/commander.log")
  passed=$(sed -n 's/^[^ ]* pass \([0-9]*\)$/\1/p' "$work/commander.log" | tail -1)
  failed=$(sed -n 's/^[^ ]* fail \([0-9]*\)$/\1/p' "$work/commander.log" | tail -1)
  [ "${added:-0}" -ge 100 ] || fail "commander: ${added:-no} packages installed from npm: $work/commander.log"
  if [ "${failed:-}" != 0 ] || [ "${passed:-0}" -lt 1300 ]; then
    fail "commander: ${passed:-no} tests passed, ${failed:-?} failed: $work/commander.log"
  fi
  say "project: commander ok ($added packages from npm, $passed tests passed)"

  # The token is masked in the tasks' homes and the settings beside it are
  # as they were; its value is nowhere in Kule's home, sealed but there.
  for layer in "$KULE_HOME"/tasks/*/home; do
    env=$layer/.env
    grep -q '^API_TOKEN=kule_secret_' "$env" || fail "the token is not masked in $env"
    for line in TOOL_ALLOW_PRIVATE_NETWORKS=false TOOL_AUTH_ENABLED=true MAX_TOKENS=4096; do
      grep -qx "$line" "$env" || fail "$line was changed in $env"
    done
  done
  ! grep -rqF -D skip -- "$token" "$KULE_HOME" || fail "the token is in Kule's home"

  # A host the policy does not name is refused, from the same task.
  if timed 120 "$kule" exec "$task" -- curl -sS -o /dev/null --max-time 20 https://example.com \
    >"$work/refused.log" 2>&1; then
    fail "a command in the task reached example.com"
  fi
  grep -q '^net: example.com:443 .*refused' "$work/refused.log" ||
    fail "Kule's proxy did not say it refused example.com: $work/refused.log"
  say "project: ok (installed, built and tested in tasks; token masked; example.com refused)"
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
  probe) probe ;;
  project) project ;;
  attest) shift; attest "$@" ;;
  # Each check that sets HOME or KULE_HOME in a shell of its own.
  all) workflows; deps; binary; (canary); (probe); (project) ;;
  *) fail "unknown check: $1" ;;
esac
