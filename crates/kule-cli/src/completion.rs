//! Shell completion. `kule completions <shell>` prints a short script that
//! holds no value from Kule's home: on each TAB it calls `kule __complete`,
//! which reads what it offers from the command line's own definitions
//! (subcommands, options, the values they take) and, for task ids, agent
//! names, group ids and extension ids, from Kule's home.
//!
//! What runs on every TAB only reads Kule's home: it starts no daemon, no
//! proxy, no agent and makes no network call, never makes the home, and
//! prints no secret, placeholder or path. It fails quietly: a home it
//! cannot read, or words it cannot place, give no completions and exit 0.
//! Every value it prints is plain (letters, digits and `-._@/=:+`), and
//! every description has its hidden characters made visible, so nothing
//! read from the home can steer the person's shell or terminal.

use std::path::{Path, PathBuf};

use clap::{Arg, Command, ValueEnum};

/// The shells Kule completes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Shell {
    Bash,
    Zsh,
    Fish,
}

/// The script `kule completions <shell>` prints.
pub(crate) fn script(shell: Shell) -> &'static str {
    match shell {
        Shell::Bash => BASH,
        Shell::Zsh => ZSH,
        Shell::Fish => FISH,
    }
}

// Each script asks `kule __complete <shell> -- <words>` and reads its
// answer: a first line `files` (complete file names) or `values`, then one
// value per line, with a tab and a description where the shell shows one.

const BASH: &str = r#"# kule completion for bash: eval "$(kule completions bash)"
_kule() {
    local -a words
    local line first=1 cur="${COMP_WORDS[COMP_CWORD]}"
    words=("${COMP_WORDS[@]:0:COMP_CWORD+1}")
    COMPREPLY=()
    while IFS= read -r line; do
        if [[ $first == 1 ]]; then
            first=0
            [[ $line == files ]] || continue
            compopt -o filenames 2>/dev/null
            while IFS= read -r line; do
                COMPREPLY+=("$line")
            done < <(compgen -f -- "$cur")
            return 0
        fi
        COMPREPLY+=("$line")
    done < <(kule __complete bash -- "${words[@]}" 2>/dev/null)
    if [[ ${#COMPREPLY[@]} -eq 1 && ${COMPREPLY[0]} == *= ]]; then
        compopt -o nospace 2>/dev/null
    fi
    return 0
}
complete -F _kule kule
"#;

const ZSH: &str = r#"#compdef kule
# kule completion for zsh: put this in a folder on $fpath as _kule, or
# source <(kule completions zsh)
_kule() {
    local -a reply values
    local line
    reply=("${(@f)$(kule __complete zsh -- "${words[@]:0:$CURRENT}" 2>/dev/null)}")
    if [[ "${reply[1]}" == files ]]; then
        _files
        return
    fi
    for line in "${(@)reply[2,-1]}"; do
        [[ -z "$line" ]] && continue
        values+=("${${line%%$'\t'*}//:/\\:}:${line#*$'\t'}")
    done
    _describe -t kule kule values -S ''
}
if [[ "$funcstack[1]" == _kule ]]; then
    _kule "$@"
else
    compdef _kule kule
fi
"#;

const FISH: &str = r#"# kule completion for fish: kule completions fish > ~/.config/fish/completions/kule.fish
function __kule_complete
    set -l words (commandline -opc) (commandline -ct)
    set -l reply (kule __complete fish -- $words 2>/dev/null)
    if test "$reply[1]" = files
        __fish_complete_path (commandline -ct)
        return
    end
    printf '%s\n' $reply[2..-1]
end
complete -c kule -f -a '(__kule_complete)'
"#;

/// What `kule __complete` answers.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Offer {
    /// The shell completes file names.
    Files,
    /// These values, each with what it is.
    Values(Vec<(String, String)>),
}

/// Answers `kule __complete <shell> -- <words>`: the words of the command
/// line up to the one being completed, `kule` first. Kule's home is the
/// one `--home` names on the line, else `KULE_HOME`'s, else the default:
/// the command's own order.
pub(crate) fn offer(cli: Command, shell: Shell, words: &[String]) -> Offer {
    let mut cmd = cli;
    cmd.build();
    let Some((current, before)) = words.split_last() else {
        return Offer::Values(Vec::new());
    };
    let before = before.get(1..).unwrap_or_default();
    let home = named_home(before).or_else(|| kule_daemon::default_home().ok());

    let mut path: Vec<String> = Vec::new();
    let mut positional = 0;
    let mut wants_value: Option<Arg> = None;
    let mut only_positional = false;
    for word in before {
        if let Some(arg) = wants_value.take() {
            let _ = arg;
            continue;
        }
        if !only_positional && word == "--" {
            only_positional = true;
        } else if !only_positional && let Some(long) = word.strip_prefix("--") {
            if !long.contains('=')
                && let Some(arg) = find_long(&cmd, long)
                && takes_value(&arg)
            {
                wants_value = Some(arg);
            }
        } else if !only_positional && word.len() > 1 && word.starts_with('-') {
            let short = word.chars().nth(1);
            if word.chars().count() == 2
                && let Some(arg) = short.and_then(|c| find_short(&cmd, c))
                && takes_value(&arg)
            {
                wants_value = Some(arg);
            }
        } else if positional == 0
            && !only_positional
            && let Some(sub) = cmd.find_subcommand(word).cloned()
        {
            path.push(sub.get_name().to_owned());
            cmd = sub;
        } else {
            positional += 1;
        }
    }

    let found = Found {
        home: home.as_deref(),
        path: &path,
    };
    let mut offered = if let Some(arg) = wants_value {
        found.values_of(&arg)
    } else if !only_positional
        && let Some((long, typed)) = current.strip_prefix("--").and_then(|l| l.split_once('='))
    {
        match find_long(&cmd, long) {
            Some(arg) => match found.values_of(&arg) {
                Offer::Values(values) => Offer::Values(
                    values
                        .into_iter()
                        .filter(|(v, _)| v.starts_with(typed))
                        .map(|(v, about)| (format!("--{long}={v}"), about))
                        .collect(),
                ),
                files => files,
            },
            None => Offer::Values(Vec::new()),
        }
    } else if !only_positional && current.starts_with('-') {
        Offer::Values(options(&cmd))
    } else {
        let mut values = Vec::new();
        if positional == 0 && cmd.has_subcommands() {
            values.extend(
                cmd.get_subcommands()
                    .filter(|s| !s.is_hide_set())
                    .map(|s| (s.get_name().to_owned(), about(s.get_about()))),
            );
        }
        if let Some(arg) = positional_at(&cmd, positional) {
            match found.values_of(&arg) {
                Offer::Values(more) => values.extend(more),
                Offer::Files if values.is_empty() => return Offer::Files,
                Offer::Files => {}
            }
        }
        Offer::Values(values)
    };
    if let Offer::Values(values) = &mut offered {
        values.retain(|(v, _)| v.starts_with(current.as_str()) && plain_value(v));
        for (value, about) in values.iter_mut() {
            *about = plain_about(about);
            // Bash breaks words at `=` and `:`: it completes what follows.
            if shell == Shell::Bash
                && let Some(at) = current.rfind(['=', ':'])
            {
                *value = value[at + 1..].to_owned();
            }
        }
    }
    offered
}

/// Values to offer, each with what it is.
type Values = Vec<(String, String)>;

/// Where to look for task ids and the like.
struct Found<'a> {
    home: Option<&'a Path>,
    /// The subcommands so far.
    path: &'a [String],
}

impl Found<'_> {
    /// What `arg` takes: the values its definition names, else what Kule's
    /// home holds for it, else file names for a path.
    fn values_of(&self, arg: &Arg) -> Offer {
        let possible: Vec<(String, String)> = arg
            .get_possible_values()
            .iter()
            .filter(|v| !v.is_hide_set())
            .map(|v| (v.get_name().to_owned(), about(v.get_help())))
            .collect();
        if !possible.is_empty() {
            return Offer::Values(possible);
        }
        let id = arg.get_id().as_str();
        let first = self.path.first().map(String::as_str);
        let values = match (first, id) {
            (_, "task") => self.read(tasks),
            (Some("run"), "agent") => self.read(agents),
            (Some("agents"), "name") => self.read(agents),
            (Some("ext"), "id") => self.read(extensions),
            (_, "group") => self.read(groups),
            (Some("limits"), "settings") => settings(),
            // clap gives an argument parsed as a path `AnyPath` unless
            // its definition names another hint.
            _ if matches!(
                arg.get_value_hint(),
                clap::ValueHint::AnyPath | clap::ValueHint::FilePath | clap::ValueHint::DirPath
            ) =>
            {
                return Offer::Files;
            }
            _ => Vec::new(),
        };
        Offer::Values(values)
    }

    /// What `read` finds in a home that is Kule's: a folder of the person's
    /// own, not a link. Nothing otherwise, and nothing on an error.
    fn read(&self, read: fn(&Path) -> Option<Values>) -> Values {
        let Some(home) = self.home else {
            return Vec::new();
        };
        let ours = std::fs::symlink_metadata(home).is_ok_and(|m| m.is_dir())
            && kule_engine::osfs::owned_by_me(home).unwrap_or(false);
        if !ours {
            return Vec::new();
        }
        read(home).unwrap_or_default()
    }
}

fn tasks(home: &Path) -> Option<Vec<(String, String)>> {
    let (infos, _) = kule_engine::arranged_told(home).ok()?;
    Some(
        infos
            .into_iter()
            .map(|info| {
                let label = kule_engine::Task::open(home, &info.id)
                    .ok()
                    .and_then(|t| t.label().ok().flatten());
                let folder = info
                    .workspace
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let about = match label {
                    Some(label) => format!("{label} · {folder}"),
                    None => folder,
                };
                (info.id, about)
            })
            .collect(),
    )
}

fn agents(home: &Path) -> Option<Vec<(String, String)>> {
    let agents = kule_acp::Agents::load(home).ok()?;
    Some(
        agents
            .listed()
            .iter()
            .map(|listed| {
                let about = if listed.held.is_some() {
                    "agent, held"
                } else {
                    "agent"
                };
                (listed.profile.name.clone(), about.to_owned())
            })
            .collect(),
    )
}

fn extensions(home: &Path) -> Option<Vec<(String, String)>> {
    let all = kule_daemon::extensions::list(home).ok()?;
    Some(
        all.into_iter()
            .map(|listed| {
                let name = listed.manifest.map(|m| m.name).unwrap_or_default();
                (listed.id, name)
            })
            .collect(),
    )
}

fn groups(home: &Path) -> Option<Vec<(String, String)>> {
    let groups = kule_engine::groups(home).ok()?;
    Some(
        groups
            .into_iter()
            .map(|g| (g.id.to_string(), g.name))
            .collect(),
    )
}

fn settings() -> Vec<(String, String)> {
    crate::scopes::setting_keys()
        .into_iter()
        .map(|(key, about)| (format!("{key}="), about))
        .collect()
}

fn find_long(cmd: &Command, long: &str) -> Option<Arg> {
    cmd.get_arguments()
        .find(|a| a.get_long() == Some(long))
        .cloned()
}

fn find_short(cmd: &Command, short: char) -> Option<Arg> {
    cmd.get_arguments()
        .find(|a| a.get_short() == Some(short))
        .cloned()
}

fn takes_value(arg: &Arg) -> bool {
    arg.get_action().takes_values()
}

/// The positional argument a word at `index` (from 0) fills: the last one
/// takes the rest when it takes many.
fn positional_at(cmd: &Command, index: usize) -> Option<Arg> {
    let positionals: Vec<&Arg> = cmd.get_positionals().filter(|a| !a.is_hide_set()).collect();
    match positionals.get(index) {
        Some(arg) => Some((*arg).clone()),
        None => positionals
            .last()
            .filter(|a| a.get_num_args().is_some_and(|n| n.max_values() > 1))
            .map(|a| (*a).clone()),
    }
}

fn options(cmd: &Command) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = cmd
        .get_arguments()
        .filter(|a| !a.is_hide_set())
        .filter_map(|a| Some((format!("--{}", a.get_long()?), about(a.get_help()))))
        .collect();
    out.push(("--help".to_owned(), "Print help".to_owned()));
    out.sort();
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

fn about(text: Option<&clap::builder::StyledStr>) -> String {
    text.map(|t| t.to_string())
        .unwrap_or_default()
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// `--home DIR` or `--home=DIR` among the words.
fn named_home(words: &[String]) -> Option<PathBuf> {
    let mut words = words.iter();
    while let Some(word) = words.next() {
        if word == "--home" {
            return words.next().map(PathBuf::from);
        }
        if let Some(home) = word.strip_prefix("--home=") {
            return Some(PathBuf::from(home));
        }
    }
    None
}

/// A value plain enough to hand a shell: what Kule's ids and names are made
/// of, and the `=` of a setting.
fn plain_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._@/=:+".contains(&b))
}

/// A description on one line, hidden characters made visible, not too long.
fn plain_about(about: &str) -> String {
    crate::paint::plain_line(about).chars().take(80).collect()
}

#[cfg(test)]
mod tests {
    use clap::{Arg, value_parser};

    use super::*;

    fn words(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_owned()).collect()
    }

    /// A command that takes a subcommand or a path first offers its
    /// subcommands, not file names, which would hide them; with no
    /// subcommands to offer, file names.
    #[test]
    fn subcommands_are_offered_before_file_names() {
        let path = || Arg::new("dir").value_parser(value_parser!(PathBuf));
        let both = Command::new("kule")
            .disable_help_subcommand(true)
            .subcommand(Command::new("sub"))
            .arg(path());
        assert_eq!(
            offer(both, Shell::Bash, &words(&["kule", ""])),
            Offer::Values(vec![("sub".to_owned(), String::new())])
        );
        let only_files = Command::new("kule").arg(path());
        assert_eq!(
            offer(only_files, Shell::Bash, &words(&["kule", ""])),
            Offer::Files
        );
    }
}
