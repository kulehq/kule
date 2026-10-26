//! The proxy inside its app container on Windows: it serves from there,
//! it reads nothing of the task's or Kule's, and it ends with the process
//! that started it. Real processes in a real container; the pipes
//! themselves are tested in `pipe.rs` on every platform.
#![cfg(windows)]

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use kule_engine::windows::appcontainer;
use kule_engine::{DEFAULT_POLICY, Policy, Proxy, Sandbox, Task};
use tempfile::TempDir;

const PROXY: &str = env!("CARGO_BIN_EXE_kule-proxy");

fn setup() -> (TempDir, TempDir, Task) {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    std::fs::write(ws.path().join("README.md"), "hello\n").unwrap();
    let task = Task::create(home.path(), ws.path()).unwrap();
    (home, ws, task)
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ok() {
        assert!(Instant::now() < deadline, "{what} did not happen in time");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The proxy Kule starts on Windows runs in an app container, serves a
/// port, hands over an authority and the roots Windows trusts, and is
/// gone once the proxy is dropped.
#[test]
fn the_proxy_serves_from_inside_its_container() {
    let (_home, _ws, task) = setup();
    let proxy = Proxy::start(&task, Policy::from_texts(DEFAULT_POLICY, None).unwrap()).unwrap();
    assert_ne!(proxy.port(), 0);
    let pid = proxy.child_pid().expect("a process of its own");
    assert!(kule_engine::osfs::win::is_app_container(pid).unwrap());
    let env: std::collections::HashMap<_, _> = proxy.env().iter().cloned().collect();
    let ca = std::fs::read_to_string(&env["NODE_EXTRA_CA_CERTS"]).unwrap();
    assert!(ca.starts_with("-----BEGIN CERTIFICATE-----"), "{ca}");
    let bundle = std::fs::read_to_string(&env["SSL_CERT_FILE"]).unwrap();
    assert!(bundle.ends_with(&ca));
    assert!(
        bundle.matches("-----BEGIN CERTIFICATE-----").count() > 1,
        "the bundle holds the roots Windows trusts before the proxy's own"
    );
    drop(proxy);
    wait_until("the process's end", || !kule_engine::osfs::alive(pid));
}

/// From inside the container the task's files and Kule's home cannot be
/// read, while the same program reads them as a plain process, and reads
/// what Windows opens to every package from inside. The program itself
/// needs no grant, wherever it is.
#[test]
fn the_container_reads_nothing_of_the_tasks_or_kules() {
    let (home, _ws, task) = setup();
    let in_task = task.copy().join("README.md");
    let in_home = home.path().join("tasks");
    let open = Path::new("C:\\Windows\\win.ini");
    let probe = |path: &Path| {
        let args = [OsStr::new("--probe"), path.as_os_str()];
        appcontainer::run(Path::new(PROXY), &args, Duration::from_secs(30))
            .unwrap_or_else(|e| panic!("{PROXY} (exists: {}): {e}", Path::new(PROXY).is_file()))
    };
    let (code, errors) = probe(&in_task);
    assert_eq!(code, Some(2), "{errors}");
    assert!(errors.contains("cannot read"), "{errors}");
    let (code, errors) = probe(&in_home);
    assert_eq!(code, Some(2), "{errors}");
    let (code, errors) = probe(open);
    assert_eq!(code, Some(0), "{errors}");
    let plain = Command::new(PROXY)
        .args(["--probe"])
        .arg(&in_task)
        .status()
        .unwrap();
    assert!(plain.success(), "as a plain process the program reads it");
    // A copy of the program under the person's own folder, which no
    // package may read, still runs in the container: Kule opens the image.
    let own = home.path().join("kule-proxy.exe");
    std::fs::copy(PROXY, &own).unwrap();
    let args = [OsStr::new("--probe"), own.as_os_str()];
    let (code, errors) = appcontainer::run(&own, &args, Duration::from_secs(30)).unwrap();
    assert_eq!(code, Some(2), "it cannot read its own file: {errors}");
    let args = [OsStr::new("--probe"), open.as_os_str()];
    let (code, errors) = appcontainer::run(&own, &args, Duration::from_secs(30)).unwrap();
    assert_eq!(code, Some(0), "{errors}");
}

/// A program in the container that touches nothing, not even its pipes,
/// and would run for a minute ends the moment the process that started
/// it is gone: the job around it ends it. (The proxy itself also ends
/// when its input ends; the job is for one that does not.)
#[test]
fn what_runs_in_the_container_ends_with_the_process_that_started_it() {
    if std::env::var_os("KULE_TEST_HOLD_CONTAINER").is_some() {
        let args = [OsStr::new("--hold"), OsStr::new("60")];
        let running = appcontainer::start(Path::new(PROXY), &args)
            .unwrap_or_else(|e| panic!("{PROXY} (exists: {}): {e}", Path::new(PROXY).is_file()));
        std::thread::sleep(Duration::from_millis(500));
        println!(
            "running-pid={} alive={}",
            running.pid(),
            kule_engine::osfs::alive(running.pid())
        );
        std::process::exit(0);
    }
    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "what_runs_in_the_container_ends_with_the_process_that_started_it",
            "--nocapture",
        ])
        .env("KULE_TEST_HOLD_CONTAINER", "1")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text
        .lines()
        .find_map(|l| l.strip_prefix("running-pid="))
        .unwrap_or_else(|| {
            panic!(
                "no pid in {text}; stderr: {}",
                String::from_utf8_lossy(&out.stderr)
            )
        });
    let (pid, alive) = line.split_once(" alive=").unwrap();
    let pid: u32 = pid.parse().unwrap();
    assert_eq!(
        alive, "true",
        "it was running before the process that started it ended"
    );
    wait_until("the program's end", || !kule_engine::osfs::alive(pid));
}

/// When the process that started the proxy ends without a word, the
/// proxy ends with it: nothing of Kule's outlives Kule.
#[test]
fn the_proxy_ends_with_the_process_that_started_it() {
    if std::env::var_os("KULE_TEST_HOLD_PROXY").is_some() {
        // The other side: a proxy started and never stopped.
        let (_home, _ws, task) = setup();
        let proxy = Proxy::start(&task, Policy::from_texts(DEFAULT_POLICY, None).unwrap()).unwrap();
        println!("proxy-pid={}", proxy.child_pid().unwrap());
        std::process::exit(0);
    }
    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "the_proxy_ends_with_the_process_that_started_it",
            "--nocapture",
        ])
        .env("KULE_TEST_HOLD_PROXY", "1")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let pid: u32 = text
        .lines()
        .find_map(|l| l.strip_prefix("proxy-pid="))
        .unwrap_or_else(|| {
            panic!(
                "no pid in {text}; stderr: {}",
                String::from_utf8_lossy(&out.stderr)
            )
        })
        .trim()
        .parse()
        .unwrap();
    wait_until("the proxy's end", || !kule_engine::osfs::alive(pid));
}

/// A profile that is not there yet is made once, however many processes
/// ask for it at the same moment: the first runs of a fresh install, from
/// the daemon and the command line at once, say. Each asker gets a
/// container that runs.
#[test]
fn a_profile_is_made_once_however_many_ask_at_once() {
    const NAME: &str = "Kule.ProxyTest";
    if std::env::var_os("KULE_TEST_ASK_AT_ONCE").is_some() {
        let args = [OsStr::new("--probe"), OsStr::new("C:\\Windows\\win.ini")];
        match appcontainer::run_in(NAME, Path::new(PROXY), &args, Duration::from_secs(30)) {
            Ok((code, errors)) => println!("asked: code={code:?} {errors}"),
            Err(e) => println!("asked: error {e}"),
        }
        std::process::exit(0);
    }
    appcontainer::remove_profile(NAME).unwrap();
    let askers: Vec<_> = (0..6)
        .map(|_| {
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "a_profile_is_made_once_however_many_ask_at_once",
                    "--nocapture",
                ])
                .env("KULE_TEST_ASK_AT_ONCE", "1")
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let said: Vec<String> = askers
        .into_iter()
        .map(|asker| {
            let out = asker.wait_with_output().unwrap();
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .find(|l| l.starts_with("asked:"))
                .unwrap_or("asked: nothing")
                .to_owned()
        })
        .collect();
    let _ = appcontainer::remove_profile(NAME);
    assert!(
        said.iter().all(|s| s.starts_with("asked: code=Some(0)")),
        "{said:#?}"
    );
}

/// Whether a task's sandbox can be made here (a desktop session and a
/// measured build); says why not otherwise, or fails where the test
/// machine requires it.
fn sandbox_available() -> bool {
    let refused = kule_engine::sandbox_refusals();
    if refused.is_empty() {
        return true;
    }
    assert!(
        std::env::var_os("KULE_TEST_REQUIRE_SANDBOX").is_none(),
        "the sandbox is required here, but: {}",
        refused.join("; ")
    );
    eprintln!("skipped: {}", refused.join("; "));
    false
}

/// End to end: a command in a task's sandbox reaches the network only
/// through the proxy in its own container, which answers it (here, with
/// the policy's refusal), and the connection is on the task's record; a
/// connection past the proxy fails at once. This is the check WP2 could
/// not make from a plain process, which the proxy's container does not
/// answer (measured).
#[test]
fn a_contained_command_reaches_the_network_only_through_the_proxy() {
    if !sandbox_available() {
        return;
    }
    let (_home, _ws, task) = setup();
    let proxy = Proxy::start(&task, Policy::from_texts(DEFAULT_POLICY, None).unwrap()).unwrap();
    let mut sandbox = Sandbox::new(&task).unwrap();
    sandbox.use_proxy(&proxy);
    let curl = |args: &[&str]| {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let out = sandbox.start("curl.exe", &args).unwrap().output().unwrap();
        format!(
            "{} {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    };
    // Through the proxy: an HTTP answer comes back, whatever the policy
    // says of the host.
    let through = curl(&[
        "-s",
        "-S",
        "-m",
        "15",
        "-o",
        "NUL",
        "-w",
        "%{http_code}",
        "http://example.com/",
    ]);
    assert!(
        through.trim_start().starts_with("200") || through.trim_start().starts_with("403"),
        "no answer through the proxy: {through}"
    );
    wait_until("the connection's record", || {
        kule_engine::connections(&task).is_ok_and(|c| !c.is_empty())
    });
    // Past it: nothing.
    let past = curl(&[
        "-s",
        "-S",
        "-m",
        "8",
        "--noproxy",
        "*",
        "-o",
        "NUL",
        "-w",
        "%{http_code}",
        "http://example.com/",
    ]);
    assert!(
        past.trim_start().starts_with("000"),
        "reached past the proxy: {past}"
    );
    drop(proxy);
}
