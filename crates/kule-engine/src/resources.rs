//! What a run's processes take as it goes: processor time and memory, read
//! from the operating system (libproc on macOS, `/proc` on Linux), and what
//! its folders take on disk. Also stopping and starting them again, which
//! is how a processor limit slows a run down.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

/// One process as last looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    pub pid: i32,
    /// Processor time since it started, user and system.
    pub cpu: Duration,
    /// Processor time of its children that ended and were waited for:
    /// work done by processes too short-lived to be seen, such as a
    /// build's compilers or a script's commands.
    pub reaped: Duration,
    /// Memory it holds: its footprint on macOS, its proportional share of
    /// what is resident on Linux.
    pub memory: u64,
}

/// `roots` that are alive and everything they started, at any depth; a
/// process whose parent is gone is not found.
pub fn tree(roots: &[i32]) -> Vec<i32> {
    let parents = parents();
    let mut found: Vec<i32> = roots
        .iter()
        .copied()
        .filter(|pid| *pid > 1 && parents.contains_key(pid))
        .collect();
    let mut frontier = found.clone();
    while let Some(parent) = frontier.pop() {
        for (&pid, &ppid) in &parents {
            if ppid == parent && !found.contains(&pid) {
                found.push(pid);
                frontier.push(pid);
            }
        }
    }
    found
}

/// What each of `pids` takes now; one that is gone or cannot be read is
/// left out.
pub fn usage(pids: &[i32]) -> Vec<Usage> {
    pids.iter().filter_map(|&pid| usage_of(pid)).collect()
}

/// Stops `pids` where they are until [`resume`]; they hold their memory.
pub fn pause(pids: &[i32]) {
    signal(pids, libc::SIGSTOP);
}

pub fn resume(pids: &[i32]) {
    signal(pids, libc::SIGCONT);
}

/// Kills `pids` at once.
pub fn kill(pids: &[i32]) {
    signal(pids, libc::SIGKILL);
}

fn signal(pids: &[i32], signal: libc::c_int) {
    let me = std::process::id() as i32;
    for &pid in pids {
        if pid > 1 && pid != me {
            // SAFETY: kill only sends a signal.
            unsafe {
                libc::kill(pid, signal);
            }
        }
    }
}

/// What `dirs` take on disk, in bytes allocated: links are not followed and
/// a file linked from several places counts once. What cannot be read is
/// left out.
pub fn disk_used(dirs: &[PathBuf]) -> u64 {
    use std::os::unix::fs::MetadataExt;
    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    let mut total = 0u64;
    for dir in dirs {
        for entry in walkdir::WalkDir::new(dir)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.nlink() > 1 && !meta.is_dir() && !seen.insert((meta.dev(), meta.ino())) {
                continue;
            }
            total = total.saturating_add(meta.blocks().saturating_mul(512));
        }
    }
    total
}

#[cfg(target_os = "macos")]
fn parents() -> BTreeMap<i32, i32> {
    use crate::ports::macos;
    macos::all_pids()
        .into_iter()
        .filter_map(|pid| Some((pid, macos::parent_of(pid)?)))
        .collect()
}

#[cfg(target_os = "macos")]
fn usage_of(pid: i32) -> Option<Usage> {
    // SAFETY: an all-zero rusage_info_v2 is valid, and the call writes one.
    let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
    let got = unsafe {
        libc::proc_pid_rusage(
            pid,
            libc::RUSAGE_INFO_V2,
            (&mut info as *mut libc::rusage_info_v2).cast(),
        )
    };
    if got != 0 {
        return None;
    }
    // In ticks of the machine's clock, not nanoseconds, on Apple silicon.
    let ticks = info.ri_user_time.saturating_add(info.ri_system_time);
    let reaped = info
        .ri_child_user_time
        .saturating_add(info.ri_child_system_time);
    Some(Usage {
        pid,
        cpu: Duration::from_nanos(ticks_to_nanos(ticks)),
        reaped: Duration::from_nanos(ticks_to_nanos(reaped)),
        memory: info.ri_phys_footprint,
    })
}

#[cfg(target_os = "macos")]
// libc's mach_timebase_info is marked for the mach2 crate; it is the same
// call.
#[allow(deprecated)]
fn ticks_to_nanos(ticks: u64) -> u64 {
    static TIMEBASE: std::sync::OnceLock<(u64, u64)> = std::sync::OnceLock::new();
    let (numer, denom) = *TIMEBASE.get_or_init(|| {
        let mut base = libc::mach_timebase_info { numer: 0, denom: 0 };
        // SAFETY: the call fills the struct it is given.
        let ok = unsafe { libc::mach_timebase_info(&mut base) } == 0;
        if ok && base.denom != 0 {
            (u64::from(base.numer), u64::from(base.denom))
        } else {
            (1, 1)
        }
    });
    (u128::from(ticks) * u128::from(numer) / u128::from(denom)) as u64
}

/// The fields of `/proc/<pid>/stat` after the name, which may hold spaces
/// and brackets itself: state first.
#[cfg(target_os = "linux")]
fn stat_fields(pid: i32) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &text[text.rfind(')')? + 1..];
    Some(rest.split_whitespace().map(str::to_owned).collect())
}

#[cfg(target_os = "linux")]
fn parents() -> BTreeMap<i32, i32> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return BTreeMap::new();
    };
    dir.flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<i32>().ok())
        .filter_map(|pid| {
            let fields = stat_fields(pid)?;
            // A zombie is over: nothing to measure, stop or wait for.
            if fields.first().map(String::as_str) == Some("Z") {
                return None;
            }
            Some((pid, fields.get(1)?.parse().ok()?))
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn usage_of(pid: i32) -> Option<Usage> {
    let fields = stat_fields(pid)?;
    // utime and stime, fields 14 and 15 of the whole line.
    let ticks: u64 = fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?;
    // cutime and cstime, fields 16 and 17: its children's, once waited for.
    let reaped: u64 = [13, 14]
        .iter()
        .map(|&i| {
            fields
                .get(i)
                .and_then(|f| f.parse::<i64>().ok())
                .unwrap_or(0)
                .max(0) as u64
        })
        .sum();
    // SAFETY: sysconf only reads a value.
    let per_sec = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as u64;
    let time = |ticks: u64| Duration::from_nanos(ticks.saturating_mul(1_000_000_000) / per_sec);
    Some(Usage {
        pid,
        cpu: time(ticks),
        reaped: time(reaped),
        memory: memory_of(pid).unwrap_or(0),
    })
}

/// Its proportional share of what is resident, so pages processes share
/// count once between them; what is resident where that cannot be read.
#[cfg(target_os = "linux")]
fn memory_of(pid: i32) -> Option<u64> {
    if let Ok(text) = std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup"))
        && let Some(kb) = text.lines().find_map(|l| {
            l.strip_prefix("Pss:")?
                .trim()
                .strip_suffix("kB")?
                .trim()
                .parse::<u64>()
                .ok()
        })
    {
        return Some(kb * 1024);
    }
    let statm = std::fs::read_to_string(format!("/proc/{pid}/statm")).ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    // SAFETY: sysconf only reads a value.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(1) as u64;
    Some(pages * page)
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};
    use std::time::Instant;

    use tempfile::TempDir;

    use super::*;

    fn gone(pid: i32) -> bool {
        tree(&[pid]).is_empty()
    }

    #[test]
    fn a_tree_is_its_root_and_what_it_started_and_can_be_paused() {
        // A shell that starts a busy child and waits for it.
        let mut root = Command::new("sh")
            .args(["-c", "sh -c 'while :; do :; done' & wait"])
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let pid = root.id() as i32;
        let started = Instant::now();
        let pids = loop {
            let pids = tree(&[pid]);
            if pids.len() >= 2 || started.elapsed() > Duration::from_secs(5) {
                break pids;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(pids.len(), 2, "{pids:?}");
        assert_eq!(pids[0], pid);
        let child = pids[1];

        let cpu = |pid| usage(&[pid])[0].cpu;
        std::thread::sleep(Duration::from_millis(300));
        let busy = cpu(child);
        assert!(busy > Duration::from_millis(100), "{busy:?}");
        assert!(usage(&[child])[0].memory > 0);

        pause(&pids);
        std::thread::sleep(Duration::from_millis(100));
        let held = cpu(child);
        std::thread::sleep(Duration::from_millis(300));
        assert!(cpu(child) - held < Duration::from_millis(20));
        resume(&pids);
        std::thread::sleep(Duration::from_millis(300));
        assert!(cpu(child) - held > Duration::from_millis(100));

        kill(&pids);
        let _ = root.wait();
        let started = Instant::now();
        while !gone(child) && started.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(gone(child));
        assert!(usage(&[pid]).is_empty() || gone(pid));
    }

    #[test]
    fn the_work_of_children_too_short_to_see_is_counted_once_they_end() {
        // A shell running one short command after another, then waiting.
        let mut root = Command::new("sh")
            .args([
                "-c",
                "i=0; while [ $i -lt 300 ]; do ls / >/dev/null; i=$((i+1)); done; sleep 30",
            ])
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let pid = root.id() as i32;
        let started = Instant::now();
        let reaped = loop {
            let reaped = usage(&[pid])[0].reaped;
            if reaped > Duration::from_millis(30) || started.elapsed() > Duration::from_secs(10) {
                break reaped;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let _ = root.kill();
        let _ = root.wait();
        assert!(reaped > Duration::from_millis(30), "{reaped:?}");
    }

    #[test]
    fn memory_is_what_a_process_holds() {
        let python = Command::new("python3")
            .args(["-c", "import sys,time; b=bytearray(200*1024*1024); sys.stdout.write('ok\\n'); sys.stdout.flush(); time.sleep(30)"])
            .stdout(Stdio::piped())
            .spawn();
        let Ok(mut python) = python else {
            eprintln!("skipped: no python3");
            return;
        };
        let mut line = String::new();
        std::io::BufRead::read_line(
            &mut std::io::BufReader::new(python.stdout.as_mut().unwrap()),
            &mut line,
        )
        .unwrap();
        let held = usage(&[python.id() as i32])[0].memory;
        let _ = python.kill();
        let _ = python.wait();
        assert!(held > 190 * 1024 * 1024, "{held}");
    }

    #[test]
    fn disk_counts_what_is_allocated_once_and_follows_no_link() {
        let dir = TempDir::new().unwrap();
        let other = TempDir::new().unwrap();
        let before = disk_used(&[dir.path().to_owned()]);
        std::fs::write(dir.path().join("a"), vec![7u8; 1 << 20]).unwrap();
        std::fs::hard_link(dir.path().join("a"), dir.path().join("b")).unwrap();
        std::fs::write(other.path().join("big"), vec![7u8; 4 << 20]).unwrap();
        std::os::unix::fs::symlink(other.path(), dir.path().join("link")).unwrap();
        let grown = disk_used(&[dir.path().to_owned()]) - before;
        assert!((1 << 20..2 << 20).contains(&grown), "{grown}");
        // A folder that is not there takes nothing.
        assert_eq!(disk_used(&[dir.path().join("none")]), 0);
    }
}
