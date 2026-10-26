//! Windows measurement spike for Kule, second round: what Microsoft's
//! `mxc-sdk` ProcessContainer holds on a real machine, before any product
//! code uses it. Not product code and not part of the workspace.
//!
//! Usage:
//!   wm probe                      host facts: platform support, backends, probe()
//!   wm run [options] -- CMDLINE   start CMDLINE in a container, relay its output
//!   wm conpty-host -- CMDLINE     run CMDLINE under a ConPTY (used *inside* a container)
//!   wm http-sink ADDR             answer every HTTP request with 200 (a stand-in proxy)
//!   wm connect ADDR               try a TCP connect from this process
//!
//! `run` options:
//!   --rw DIR / --ro DIR / --deny DIR   filesystem policy (repeatable)
//!   --enum DIR                         list entries without reading (repeatable)
//!   --capture FILE.json                record access denials (learning mode, still blocking)
//!   --env K=V                          environment entry (repeatable)
//!   --env-inherit                      inherit the backend default environment
//!   --env-default                      send no environment at all (backend default)
//!   --net deny|allow|proxy:PORT[:PEER]|proxy-loopback:PORT
//!   --ui                               leave UI enabled (default: disabled)
//!   --cwd DIR                          working directory
//!   --timeout MS                       timeout (default 60000)
//!   --name NAME                        container name
//!   --probe-port PORT [--after MS]     from the host, connect to 127.0.0.1:PORT after MS
//!   --linger NAME.exe                  after exit, list processes with that image name
use anyhow::{Context, Result, bail};
use mxc_sdk::v1::configs::{
    CaptureDenials, CaptureDenialsMode, ProcessContainerConfig, ProcessContainerFilesystem,
    ProcessContainerNetwork,
};
use mxc_sdk::v1::policy::{
    FilesystemPolicy, NetworkAction, NetworkEgressPolicy, NetworkIngressPolicy, NetworkPolicy,
    NetworkRuntimeConfig, UiPolicy,
};
use mxc_sdk::v1::{self, ContainerRequest, Containment, WaitResult};
use std::io::{Read, Write};
use std::time::Duration;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("probe") => probe(),
        Some("run") => run(&args[1..]),
        Some("conpty-host") => conpty::host(&args[1..]),
        Some("http-sink") => http_sink(args.get(1).context("ADDR")?),
        Some("connect") => {
            let addr: std::net::SocketAddr = args.get(1).context("ADDR")?.parse()?;
            match std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(4)) {
                Ok(_) => println!("connected {addr}"),
                Err(e) => println!("connect {addr} failed: {e}"),
            }
            Ok(())
        }
        _ => bail!(
            "usage: wm probe | run [options] -- CMDLINE | conpty-host -- CMDLINE | http-sink ADDR | connect ADDR"
        ),
    }
}

fn probe() -> Result<()> {
    let support = v1::platform_support();
    println!("platform supported: {}", support.is_supported);
    if let Some(reason) = &support.reason {
        println!("reason: {reason}");
    }
    println!("available methods: {:?}", support.available_methods);
    for backend in v1::available_backends() {
        println!("backend: {} tier={:?}", backend.backend, backend.tier);
        println!("  capabilities: {:?}", backend.capabilities);
        for warning in backend.warnings {
            println!("  warning: {warning}");
        }
    }
    let request = ContainerRequest::new("cmd.exe /d /s /c \"echo probe\"");
    match v1::probe(Some(&request)) {
        Ok(out) => {
            println!("probe tier: {:?}", out.tier);
            println!(
                "probe needs_dacl_augmentation: {:?}",
                out.needs_dacl_augmentation
            );
            println!("probe facts: {:#?}", out.probes);
            for warning in out.warnings {
                println!("probe warning: {warning}");
            }
            if let Some(error) = out.error {
                println!("probe error: {error}");
            }
        }
        Err(e) => println!("probe failed: {e}"),
    }
    Ok(())
}

struct Opts {
    rw: Vec<String>,
    ro: Vec<String>,
    deny: Vec<String>,
    enumerate: Vec<String>,
    capture: Option<String>,
    env: Vec<(String, String)>,
    env_inherit: bool,
    env_default: bool,
    net: String,
    ui: bool,
    cwd: Option<String>,
    timeout: u32,
    name: Option<String>,
    probe_port: Option<u16>,
    after: u64,
    linger: Option<String>,
    cmd: String,
}

fn parse(args: &[String]) -> Result<Opts> {
    let mut o = Opts {
        rw: vec![],
        ro: vec![],
        deny: vec![],
        enumerate: vec![],
        capture: None,
        env: vec![],
        env_inherit: false,
        env_default: false,
        net: "deny".into(),
        ui: false,
        cwd: None,
        timeout: 60_000,
        name: None,
        probe_port: None,
        after: 3000,
        linger: None,
        cmd: String::new(),
    };
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let mut next = || -> Result<String> {
            i += 1;
            args.get(i)
                .cloned()
                .with_context(|| format!("{a} needs a value"))
        };
        match a {
            "--rw" => o.rw.push(next()?),
            "--ro" => o.ro.push(next()?),
            "--deny" => o.deny.push(next()?),
            "--enum" => o.enumerate.push(next()?),
            "--capture" => o.capture = Some(next()?),
            "--env" => {
                let kv = next()?;
                let (k, v) = kv.split_once('=').context("--env K=V")?;
                o.env.push((k.to_string(), v.to_string()));
            }
            "--env-inherit" => o.env_inherit = true,
            "--env-default" => o.env_default = true,
            "--net" => o.net = next()?,
            "--ui" => o.ui = true,
            "--cwd" => o.cwd = Some(next()?),
            "--timeout" => o.timeout = next()?.parse()?,
            "--name" => o.name = Some(next()?),
            "--probe-port" => o.probe_port = Some(next()?.parse()?),
            "--after" => o.after = next()?.parse()?,
            "--linger" => o.linger = Some(next()?),
            "--" => {
                o.cmd = args[i + 1..].join(" ");
                break;
            }
            other => bail!("unknown option {other}"),
        }
        i += 1;
    }
    if o.cmd.is_empty() {
        bail!("no command after --");
    }
    Ok(o)
}

fn network(spec: &str) -> Result<(Option<NetworkPolicy>, Option<String>)> {
    let action = |allow: bool| {
        if allow {
            NetworkAction::Allow
        } else {
            NetworkAction::Deny
        }
    };
    Ok(match spec.split(':').collect::<Vec<_>>().as_slice() {
        ["deny"] => (
            Some(NetworkPolicy {
                egress: Some(NetworkEgressPolicy {
                    default: Some(action(false)),
                    allow: None,
                    deny: None,
                }),
                ingress: Some(NetworkIngressPolicy {
                    default: Some(action(false)),
                    host_loopback: Some(action(false)),
                }),
                runtime_config: None,
            }),
            None,
        ),
        ["allow"] => (
            Some(NetworkPolicy {
                egress: Some(NetworkEgressPolicy {
                    default: Some(action(true)),
                    allow: None,
                    deny: None,
                }),
                ingress: Some(NetworkIngressPolicy {
                    default: Some(action(true)),
                    host_loopback: Some(action(false)),
                }),
                runtime_config: None,
            }),
            None,
        ),
        ["proxy", port, peer] => (
            Some(NetworkPolicy {
                egress: Some(NetworkEgressPolicy {
                    default: Some(action(false)),
                    allow: None,
                    deny: None,
                }),
                ingress: Some(NetworkIngressPolicy {
                    default: Some(action(true)),
                    host_loopback: Some(action(false)),
                }),
                runtime_config: Some(NetworkRuntimeConfig {
                    network_proxy: Some(format!("http://127.0.0.1:{port}")),
                }),
            }),
            Some(peer.to_string()),
        ),
        ["proxy-loopback", port] => (
            Some(NetworkPolicy {
                egress: Some(NetworkEgressPolicy {
                    default: Some(action(false)),
                    allow: None,
                    deny: None,
                }),
                ingress: Some(NetworkIngressPolicy {
                    default: Some(action(true)),
                    host_loopback: Some(action(true)),
                }),
                runtime_config: Some(NetworkRuntimeConfig {
                    network_proxy: Some(format!("http://127.0.0.1:{port}")),
                }),
            }),
            None,
        ),
        _ => bail!("--net deny|allow|proxy:PORT:PEER|proxy-loopback:PORT"),
    })
}

fn run(args: &[String]) -> Result<()> {
    let o = parse(args)?;
    let (network, peer) = network(&o.net)?;
    let mut config = ProcessContainerConfig::default();
    if let Some(path) = &o.capture {
        let mut c = CaptureDenials::default();
        c.mode = CaptureDenialsMode::Block;
        c.output_path = Some(path.clone());
        config.capture_denials = Some(c);
    }
    if !o.enumerate.is_empty() {
        let mut fs = ProcessContainerFilesystem::default();
        fs.enumerate_paths = o.enumerate.clone();
        config.filesystem = Some(fs);
    }
    if let Some(peer) = peer {
        let mut network = ProcessContainerNetwork::default();
        network.allowed_proxy_peer = Some(peer);
        config.network = Some(network);
    }
    let environment = if o.env_default {
        None
    } else {
        Some(o.env.clone())
    };
    let request = ContainerRequest {
        filesystem: Some(FilesystemPolicy {
            readwrite_paths: o.rw.clone(),
            readonly_paths: o.ro.clone(),
            denied_paths: o.deny.clone(),
            clear_policy_on_exit: None,
        }),
        network,
        ui: Some(UiPolicy {
            disable: !o.ui,
            ..UiPolicy::default()
        }),
        timeout_ms: Some(o.timeout),
        containment: Containment::ProcessContainer(config),
        container_name: o.name.clone(),
        working_directory: o.cwd.clone(),
        environment,
        inherit_default_environment: Some(o.env_inherit),
        ..ContainerRequest::new(o.cmd.clone())
    };
    let started = std::time::Instant::now();
    let mut process = match v1::spawn(request, Default::default()) {
        Ok(p) => p,
        Err(e) => {
            println!("spawn failed: code={} message={}", e.code, e.message);
            if let Some(n) = e.native_code {
                println!("  native: {n}");
            }
            if let Some(r) = e.remediation {
                println!("  remediation: {r}");
            }
            return Ok(());
        }
    };
    let stdin_keeper = process.take_stdin();
    println!("[pid {} started in {:?}]", process.id(), started.elapsed());
    for warning in process.warnings() {
        println!("[warning] {warning}");
    }
    let out = process.take_stdout().map(|mut r| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = r.read_to_end(&mut buf);
            buf
        })
    });
    let err = process.take_stderr().map(|mut r| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = r.read_to_end(&mut buf);
            buf
        })
    });
    if let Some(port) = o.probe_port {
        std::thread::sleep(Duration::from_millis(o.after));
        let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse()?;
        match std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(4)) {
            Ok(mut s) => {
                let _ = s.write_all(b"GET / HTTP/1.0\r\n\r\n");
                let mut buf = [0u8; 64];
                let n = s.read(&mut buf).unwrap_or(0);
                println!(
                    "[host] connected to {addr}; read {n} bytes: {:?}",
                    String::from_utf8_lossy(&buf[..n])
                );
            }
            Err(e) => println!("[host] connect to {addr} failed: {e}"),
        }
        let _ = process.kill();
    }
    let outcome = process.wait()?;
    drop(stdin_keeper);
    println!("[outcome {:?} after {:?}]", outcome, started.elapsed());
    if let Some(t) = out {
        let bytes = t.join().unwrap_or_default();
        println!(
            "--- stdout ({} bytes)\n{}",
            bytes.len(),
            String::from_utf8_lossy(&bytes)
        );
    }
    if let Some(t) = err {
        let bytes = t.join().unwrap_or_default();
        println!(
            "--- stderr ({} bytes)\n{}",
            bytes.len(),
            String::from_utf8_lossy(&bytes)
        );
    }
    for warning in process.warnings() {
        println!("[warning after] {warning}");
    }
    if let Some(name) = o.linger {
        std::thread::sleep(Duration::from_secs(2));
        let list = std::process::Command::new("tasklist")
            .args(["/FI", &format!("IMAGENAME eq {name}"), "/NH"])
            .output()?;
        println!(
            "--- tasklist {name} 2s after exit:\n{}",
            String::from_utf8_lossy(&list.stdout).trim()
        );
    }
    let _ = matches!(outcome, WaitResult::TimedOut);
    Ok(())
}

fn http_sink(addr: &str) -> Result<()> {
    let listener = std::net::TcpListener::bind(addr)?;
    println!("sink listening {}", listener.local_addr()?);
    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(s) => s,
            Err(e) => {
                println!("accept failed: {e}");
                continue;
            }
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buf = [0u8; 2048];
        let n = stream.read(&mut buf).unwrap_or(0);
        let first = String::from_utf8_lossy(&buf[..n])
            .lines()
            .next()
            .unwrap_or("")
            .to_string();
        println!("request from {:?}: {first}", stream.peer_addr().ok());
        let body = b"sink";
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(body);
    }
    Ok(())
}

mod conpty {
    //! A minimal ConPTY host: creates a pseudo console, starts CMDLINE on it,
    //! relays our stdin to the console input and the console output to our
    //! stdout. Measures whether ConPTY works inside a ProcessContainer.
    use anyhow::{Result, bail};
    use std::io::{Read, Write};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Console::{COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON};
    use windows::Win32::System::Pipes::CreatePipe;
    use windows::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
        GetExitCodeProcess, INFINITE, InitializeProcThreadAttributeList,
        LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_INFORMATION,
        STARTUPINFOEXW, UpdateProcThreadAttribute, WaitForSingleObject,
    };
    use windows::core::{PCWSTR, PWSTR};

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub fn host(args: &[String]) -> Result<()> {
        let cmd = match args.iter().position(|a| a == "--") {
            Some(p) => args[p + 1..].join(" "),
            None => bail!("conpty-host -- CMDLINE"),
        };
        unsafe {
            let (mut in_read, mut in_write) = (HANDLE::default(), HANDLE::default());
            let (mut out_read, mut out_write) = (HANDLE::default(), HANDLE::default());
            CreatePipe(&mut in_read, &mut in_write, None, 0)?;
            CreatePipe(&mut out_read, &mut out_write, None, 0)?;
            let size = COORD { X: 100, Y: 30 };
            let pty = match CreatePseudoConsole(size, in_read, out_write, 0) {
                Ok(p) => p,
                Err(e) => {
                    println!("CreatePseudoConsole failed: {e}");
                    return Ok(());
                }
            };
            println!("CreatePseudoConsole ok");
            let _ = CloseHandle(in_read);
            let _ = CloseHandle(out_write);

            let mut size_needed = 0usize;
            let _ = InitializeProcThreadAttributeList(None, 1, None, &mut size_needed);
            let mut buf = vec![0u8; size_needed];
            let list = LPPROC_THREAD_ATTRIBUTE_LIST(buf.as_mut_ptr() as *mut _);
            InitializeProcThreadAttributeList(Some(list), 1, None, &mut size_needed)?;
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                Some(pty.0 as *const _),
                std::mem::size_of::<HPCON>(),
                None,
                None,
            )?;
            let mut si = STARTUPINFOEXW::default();
            si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
            si.lpAttributeList = list;
            let mut cmdline = wide(&cmd);
            let mut pi = PROCESS_INFORMATION::default();
            let created = CreateProcessW(
                PCWSTR::null(),
                Some(PWSTR(cmdline.as_mut_ptr())),
                None,
                None,
                false,
                EXTENDED_STARTUPINFO_PRESENT,
                None,
                PCWSTR::null(),
                &si.StartupInfo,
                &mut pi,
            );
            if let Err(e) = created {
                println!("CreateProcessW under ConPTY failed: {e}");
                ClosePseudoConsole(pty);
                return Ok(());
            }
            println!("shell pid {}", pi.dwProcessId);

            let out_handle = out_read.0 as isize;
            let reader = std::thread::spawn(move || {
                let mut file = std::fs::File::from(
                    std::os::windows::io::OwnedHandle::from_raw_handle(out_handle as *mut _),
                );
                let mut stdout = std::io::stdout();
                let mut buf = [0u8; 4096];
                loop {
                    match file.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let _ = stdout.write_all(&buf[..n]);
                            let _ = stdout.flush();
                        }
                    }
                }
            });
            let in_handle = in_write.0 as isize;
            std::thread::spawn(move || {
                let mut file = std::fs::File::from(
                    std::os::windows::io::OwnedHandle::from_raw_handle(in_handle as *mut _),
                );
                let mut stdin = std::io::stdin();
                let mut buf = [0u8; 1024];
                loop {
                    match stdin.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if file.write_all(&buf[..n]).is_err() {
                                break;
                            }
                        }
                    }
                }
                // Keep the console input open: closing it ends the pseudo console
                // and the shell with it (STATUS_CONTROL_C_EXIT).
                std::mem::forget(file);
            });
            let waited = WaitForSingleObject(pi.hProcess, INFINITE);
            let mut code = 0u32;
            let _ = GetExitCodeProcess(pi.hProcess, &mut code);
            ClosePseudoConsole(pty);
            let _ = reader.join();
            DeleteProcThreadAttributeList(list);
            let _ = CloseHandle(pi.hProcess);
            let _ = CloseHandle(pi.hThread);
            println!(
                "\nshell exited: wait={} code={code}",
                waited == WAIT_OBJECT_0
            );
        }
        Ok(())
    }
}
