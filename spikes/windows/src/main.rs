//! Windows measurement spike for Kule: what AppContainer and Job Objects hold.
//! Not product code. Usage:
//!   winspike run --name NAME [--grant DIR]... [--cap SID]... [--job] [--cwd DIR] -- CMDLINE
//!   winspike sid NAME
//!   winspike delete NAME
use anyhow::{Context, Result, bail};
use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Security::Authorization::{ConvertSidToStringSidW, ConvertStringSidToSidW};
use windows::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeleteAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
};
use windows::Win32::Security::{PSID, SECURITY_CAPABILITIES, SID_AND_ATTRIBUTES};
const SE_GROUP_ENABLED: u32 = 4;
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
};
use windows::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess,
    InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
    PROCESS_INFORMATION, STARTUPINFOEXW, UpdateProcThreadAttribute, WaitForSingleObject, INFINITE,
    CREATE_SUSPENDED, ResumeThread, CREATE_NEW_PROCESS_GROUP,
};
use windows::core::{PCWSTR, PWSTR};

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

fn sid_string(sid: PSID) -> Result<String> {
    let mut out = PWSTR::null();
    unsafe { ConvertSidToStringSidW(sid, &mut out)? };
    let s = unsafe { out.to_string()? };
    Ok(s)
}

fn profile_sid(name: &str) -> Result<PSID> {
    let w = wide(name);
    match unsafe { DeriveAppContainerSidFromAppContainerName(PCWSTR(w.as_ptr())) } {
        Ok(sid) => Ok(sid),
        Err(e) => bail!("derive sid: {e}"),
    }
}

fn ensure_profile(name: &str) -> Result<PSID> {
    let w = wide(name);
    let disp = wide("Kule spike");
    match unsafe { CreateAppContainerProfile(PCWSTR(w.as_ptr()), PCWSTR(disp.as_ptr()), PCWSTR(disp.as_ptr()), None) } {
        Ok(sid) => Ok(sid),
        // ERROR_ALREADY_EXISTS -> derive
        Err(_) => profile_sid(name),
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("sid") => {
            let sid = profile_sid(&args[1])?;
            println!("{}", sid_string(sid)?);
        }
        Some("delete") => {
            let w = wide(&args[1]);
            unsafe { DeleteAppContainerProfile(PCWSTR(w.as_ptr()))? };
            println!("deleted");
        }
        Some("run") => run(&args[1..])?,
        Some("sleep") => std::thread::sleep(std::time::Duration::from_secs(args[1].parse()?)),
        Some("listen") => {
            let l = std::net::TcpListener::bind(("127.0.0.1", args[1].parse::<u16>()?))?;
            println!("listening {}", l.local_addr()?);
            l.set_nonblocking(true)?;
            let end = std::time::Instant::now() + std::time::Duration::from_secs(3);
            while std::time::Instant::now() < end { if l.accept().is_ok() { println!("accepted"); } std::thread::sleep(std::time::Duration::from_millis(50)); }
        }
        Some("connect") => {
            let addr = format!("{}:{}", args[1], args[2]);
            let a: std::net::SocketAddr = addr.parse()?;
            match std::net::TcpStream::connect_timeout(&a, std::time::Duration::from_secs(4)) {
                Ok(_) => println!("connected {addr}"),
                Err(e) => { println!("refused {addr}: {e}"); std::process::exit(2); }
            }
        }
        Some("whoami") => {
            let out = std::process::Command::new("whoami").args(["/groups", "/fo", "csv"]).output()?;
            let text = String::from_utf8_lossy(&out.stdout);
            let ac = text.lines().any(|l| l.contains("S-1-15-2-"));
            let il = text.lines().find(|l| l.contains("Mandatory Level")).map(|l| l.split('"').nth(1).unwrap_or("?").to_string()).unwrap_or("?".into());
            println!("appcontainer={ac} integrity={il} session={}", std::env::var("SESSIONNAME").unwrap_or_default());
        }
        Some("sddl") => {
            use windows::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT, ConvertSecurityDescriptorToStringSecurityDescriptorW};
            use windows::Win32::Security::{DACL_SECURITY_INFORMATION, LABEL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};
            use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
            use windows::Win32::Foundation::GENERIC_READ;
            let w = wide(&args[1]);
            let h = unsafe { CreateFileW(PCWSTR(w.as_ptr()), 0x00020000 /*READ_CONTROL*/ | GENERIC_READ.0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(FILE_FLAG_BACKUP_SEMANTICS.0), None) }.context("open")?;
            let mut sd = PSECURITY_DESCRIPTOR::default();
            let r = unsafe { GetSecurityInfo(h, SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION, None, None, None, None, Some(&mut sd)) };
            if r.is_err() { bail!("GetSecurityInfo failed: {r:?}"); }
            let mut out = PWSTR::null();
            unsafe { ConvertSecurityDescriptorToStringSecurityDescriptorW(sd, 1, OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION, &mut out, None)? };
            println!("{}", unsafe { out.to_string()? });
        }
        Some("token") => {
            use windows::Win32::Security::{GetTokenInformation, TokenIntegrityLevel, TokenIsAppContainer, TOKEN_QUERY, TOKEN_MANDATORY_LABEL, GetSidSubAuthority, GetSidSubAuthorityCount};
            use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
            let mut t = HANDLE::default();
            unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut t)? };
            let mut is_ac: u32 = 0; let mut n = 0u32;
            unsafe { GetTokenInformation(t, TokenIsAppContainer, Some(&mut is_ac as *mut _ as *mut c_void), 4, &mut n)? };
            let mut buf = vec![0u8; 64]; let mut n = 0u32;
            unsafe { GetTokenInformation(t, TokenIntegrityLevel, Some(buf.as_mut_ptr() as *mut c_void), 64, &mut n)? };
            let label = unsafe { &*(buf.as_ptr() as *const TOKEN_MANDATORY_LABEL) };
            let il = unsafe { *GetSidSubAuthority(label.Label.Sid, (*GetSidSubAuthorityCount(label.Label.Sid) - 1) as u32) };
            let nul = std::fs::OpenOptions::new().read(true).write(true).open("NUL").map(|_| "ok".to_string()).unwrap_or_else(|e| e.to_string());
            println!("appcontainer={} integrity=0x{il:x} nul={nul}", is_ac != 0);
        }
        Some("probe") => {
            // args: copy dir. Reports each operation's io error precisely.
            let copy = std::path::PathBuf::from(&args[1]);
            let temp = std::env::var("TEMP").unwrap_or_default();
            let r = |label: &str, res: std::io::Result<String>| println!("{label:<34} {}", match res { Ok(s) => format!("ok {s}"), Err(e) => format!("ERR {e}") });
            r("read_dir copy", std::fs::read_dir(&copy).map(|d| d.count().to_string()));
            r("read_dir temp", std::fs::read_dir(&temp).map(|d| d.count().to_string()));
            r("read_dir Git\\cmd", std::fs::read_dir("C:\\Program Files\\Git\\cmd").map(|d| d.count().to_string()));
            r("read_dir System32\\drivers\\etc", std::fs::read_dir("C:\\Windows\\System32\\drivers\\etc").map(|d| d.count().to_string()));
            r("metadata copy\\ws.exe", std::fs::metadata(copy.join("ws.exe")).map(|m| m.len().to_string()));
            r("open copy\\ws.exe read", std::fs::File::open(copy.join("ws.exe")).map(|_| String::new()));
            r("open NUL read", std::fs::File::open("NUL").map(|_| String::new()));
            r("open NUL write", std::fs::OpenOptions::new().write(true).open("NUL").map(|_| String::new()));
            r("open CONOUT$", std::fs::OpenOptions::new().write(true).open("CONOUT$").map(|_| String::new()));
            r("write temp file", std::fs::write(std::path::Path::new(&temp).join("probe.txt"), "x").map(|_| String::new()));
            r("create copy\\sub dir", std::fs::create_dir_all(copy.join("sub")).map(|_| String::new()));
            let inp = std::fs::File::open(copy.join("file.txt"));
            let spawn = |exe: &str, args: &[&str]| -> std::io::Result<String> {
                let mut c = std::process::Command::new(exe);
                c.args(args).stdin(match inp.as_ref() { Ok(f) => std::process::Stdio::from(f.try_clone()?), Err(_) => std::process::Stdio::inherit() });
                c.stdout(std::process::Stdio::from(std::fs::File::create(copy.join("spawn.out"))?)).stderr(std::process::Stdio::from(std::fs::File::create(copy.join("spawn.err"))?));
                let st = c.status()?;
                let out = std::fs::read_to_string(copy.join("spawn.out")).unwrap_or_default();
                let err = std::fs::read_to_string(copy.join("spawn.err")).unwrap_or_default();
                Ok(format!("exit {:?} out={:?} err={:?}", st.code(), out.trim().chars().take(60).collect::<String>(), err.trim().chars().take(80).collect::<String>()))
            };
            r("spawn cmd /c echo hi", spawn("cmd", &["/c", "echo hi"]));
            r("spawn copy\\c.exe --version", spawn(copy.join("c.exe").to_str().unwrap(), &["--version"]));
            r("spawn copy\\ws.exe token", spawn(copy.join("ws.exe").to_str().unwrap(), &["token"]));
            r("spawn System32 curl --version", spawn("C:\\Windows\\System32\\curl.exe", &["--version"]));
            r("spawn mingw git --version", spawn("C:\\Program Files\\Git\\mingw64\\bin\\git.exe", &["--version"]));
            r("spawn node --version", spawn("C:\\Program Files\\nodejs\\node.exe", &["--version"]));
            let profile = std::env::var("USERPROFILE").unwrap_or_default();
            r("read profile secret", std::fs::read_to_string(format!("{profile}\\kule-secret.txt")).map(|s| s.trim().to_string()));
            r("write profile esc.txt", std::fs::write(format!("{profile}\\esc.txt"), "x").map(|_| String::new()));
            r("read_dir profile", std::fs::read_dir(&profile).map(|d| d.count().to_string()));
            r("read other task copy", std::fs::read_to_string(copy.parent().unwrap().join("other-task").join("file.txt")).map(|s| s.trim().to_string()));
            r("spawn Git cmd\\git.exe --version", spawn("C:\\Program Files\\Git\\cmd\\git.exe", &["--version"]));
            r("tcp connect 127.0.0.1:18080", std::net::TcpStream::connect_timeout(&"127.0.0.1:18080".parse().unwrap(), std::time::Duration::from_secs(3)).map(|_| String::new()));
            r("tcp listen 127.0.0.1:18082", std::net::TcpListener::bind("127.0.0.1:18082").map(|_| String::new()));
        }
        Some("logon-sid") => println!("{}", logon_sid()?),
        Some("pipe-server") => pipe_server(&args[1], args.get(2).map(String::as_str) == Some("--open"))?,
        Some("pipe-client") => pipe_client(&args[1])?,
        Some("clone") => clone_tree(std::path::Path::new(&args[1]), std::path::Path::new(&args[2]))?,
        Some("breakaway") => {
            // try to start a sleeper outside the job
            use windows::Win32::System::Threading::{CREATE_BREAKAWAY_FROM_JOB, DETACHED_PROCESS, STARTUPINFOW};
            let exe = std::env::current_exe()?;
            let mut cmd_w = wide(&format!("\"{}\" sleep 25", exe.display()));
            let si = STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, ..Default::default() };
            let mut pi = PROCESS_INFORMATION::default();
            let flags = if args.get(1).map(String::as_str) == Some("detached") { DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP } else { CREATE_BREAKAWAY_FROM_JOB | DETACHED_PROCESS };
            match unsafe { CreateProcessW(PCWSTR::null(), Some(PWSTR(cmd_w.as_mut_ptr())), None, None, false, flags, None, PCWSTR::null(), &si, &mut pi) } {
                Ok(()) => println!("started pid {}", pi.dwProcessId),
                Err(e) => println!("refused: {e}"),
            }
        }
        _ => bail!("usage: winspike run|sid|delete|sleep|listen|connect|whoami|sddl|token|probe|logon-sid|pipe-server|pipe-client|clone|breakaway ..."),
    }
    Ok(())
}

fn run(args: &[String]) -> Result<()> {
    let mut name = String::new();
    let mut grants = Vec::new();
    let mut caps = Vec::new();
    let mut job = false;
    let mut restricted: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut cmdline = String::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--name" => { name = args[i + 1].clone(); i += 2; }
            "--grant" => { grants.push(args[i + 1].clone()); i += 2; }
            "--cap" => { caps.push(args[i + 1].clone()); i += 2; }
            "--cwd" => { cwd = Some(args[i + 1].clone()); i += 2; }
            "--job" => { job = true; i += 1; }
            "--restricted" => { restricted = Some(args[i + 1].clone()); i += 2; }
            "--" => { cmdline = args[i + 1..].join(" "); break; }
            other => bail!("unknown arg {other}"),
        }
    }
    if cmdline.is_empty() { bail!("-- CMDLINE is required"); }
    if let Some(level) = restricted { return run_restricted(&level, &grants, cwd.as_deref(), job, &cmdline); }
    if name.is_empty() { bail!("--name is required"); }
    let sid = ensure_profile(&name)?;
    let sid_s = sid_string(sid)?;
    eprintln!("[spike] container {name} = {sid_s}");
    for g in &grants {
        let st = std::process::Command::new("icacls")
            .args([g, "/grant", &format!("*{sid_s}:(OI)(CI)(F)"), "/q"])
            .status().context("icacls")?;
        if !st.success() { bail!("icacls failed for {g}"); }
    }
    // Capability SIDs
    let mut cap_sids: Vec<PSID> = Vec::new();
    for c in &caps {
        let w = wide(c);
        let mut p = PSID::default();
        unsafe { ConvertStringSidToSidW(PCWSTR(w.as_ptr()), &mut p)? };
        cap_sids.push(p);
    }
    let mut cap_attrs: Vec<SID_AND_ATTRIBUTES> = cap_sids
        .iter()
        .map(|p| SID_AND_ATTRIBUTES { Sid: *p, Attributes: SE_GROUP_ENABLED as u32 })
        .collect();
    let mut sc = SECURITY_CAPABILITIES {
        AppContainerSid: sid,
        Capabilities: if cap_attrs.is_empty() { std::ptr::null_mut() } else { cap_attrs.as_mut_ptr() },
        CapabilityCount: cap_attrs.len() as u32,
        Reserved: 0,
    };

    // Attribute list
    let mut size: usize = 0;
    let _ = unsafe { InitializeProcThreadAttributeList(None, 1, None, &mut size) };
    let mut buf = vec![0u8; size];
    let list = LPPROC_THREAD_ATTRIBUTE_LIST(buf.as_mut_ptr() as *mut c_void);
    unsafe { InitializeProcThreadAttributeList(Some(list), 1, None, &mut size)? };
    unsafe {
        UpdateProcThreadAttribute(
            list,
            0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            Some(&mut sc as *mut _ as *const c_void),
            std::mem::size_of::<SECURITY_CAPABILITIES>(),
            None,
            None,
        )?
    };

    let mut si = STARTUPINFOEXW::default();
    si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    si.lpAttributeList = list;
    let mut pi = PROCESS_INFORMATION::default();
    let mut cmd_w = wide(&cmdline);
    let cwd_w = cwd.as_deref().map(wide);
    let flags = EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP;
    let started = std::time::Instant::now();
    unsafe {
        CreateProcessW(
            PCWSTR::null(),
            Some(PWSTR(cmd_w.as_mut_ptr())),
            None,
            None,
            false,
            flags,
            None,
            cwd_w.as_ref().map(|w| PCWSTR(w.as_ptr())).unwrap_or(PCWSTR::null()),
            &si.StartupInfo,
            &mut pi,
        )
        .context("CreateProcessW")?
    };
    let mut hjob = HANDLE::default();
    if job {
        hjob = unsafe { CreateJobObjectW(None, PCWSTR::null())? };
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        unsafe {
            SetInformationJobObject(
                hjob,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )?;
            AssignProcessToJobObject(hjob, pi.hProcess)?;
        }
    }
    unsafe { ResumeThread(pi.hThread) };
    let w = unsafe { WaitForSingleObject(pi.hProcess, INFINITE) };
    if w != WAIT_OBJECT_0 { bail!("wait failed"); }
    let mut code = 0u32;
    unsafe { GetExitCodeProcess(pi.hProcess, &mut code)? };
    eprintln!("[spike] exit {code} after {:?}", started.elapsed());
    unsafe {
        DeleteProcThreadAttributeList(list);
        let _ = CloseHandle(pi.hThread);
        let _ = CloseHandle(pi.hProcess);
        if job { let _ = CloseHandle(hjob); }
    }
    std::process::exit(code as i32);
}


// ---------- restricted token (Chromium-style) ----------
use windows::Win32::Security::{
    CreateRestrictedToken, GetTokenInformation, SetTokenInformation, TokenGroups, TokenIntegrityLevel, TokenUser,
    TOKEN_ALL_ACCESS, TOKEN_GROUPS, TOKEN_MANDATORY_LABEL, TOKEN_USER, DISABLE_MAX_PRIVILEGE, TOKEN_ACCESS_MASK,
};
use windows::Win32::System::Threading::{CreateProcessAsUserW, GetCurrentProcess, OpenProcessToken, STARTUPINFOW};
const SE_GROUP_LOGON_ID: u32 = 0xC0000000;

fn string_sid(s: &str) -> Result<PSID> {
    let w = wide(s);
    let mut p = PSID::default();
    unsafe { ConvertStringSidToSidW(PCWSTR(w.as_ptr()), &mut p)? };
    Ok(p)
}

fn token_info(t: HANDLE, class: windows::Win32::Security::TOKEN_INFORMATION_CLASS) -> Result<Vec<u8>> {
    let mut n = 0u32;
    let _ = unsafe { GetTokenInformation(t, class, None, 0, &mut n) };
    let mut buf = vec![0u8; n as usize];
    unsafe { GetTokenInformation(t, class, Some(buf.as_mut_ptr() as *mut c_void), n, &mut n)? };
    Ok(buf)
}

fn logon_sid() -> Result<String> {
    let mut t = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ACCESS_MASK(0x0008), &mut t)? };
    let buf = token_info(t, TokenGroups)?;
    let groups = unsafe { &*(buf.as_ptr() as *const TOKEN_GROUPS) };
    let arr = unsafe { std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize) };
    for g in arr {
        if g.Attributes & SE_GROUP_LOGON_ID == SE_GROUP_LOGON_ID { return sid_string(g.Sid); }
    }
    bail!("no logon sid")
}

fn run_restricted(level: &str, grants: &[String], cwd: Option<&str>, job: bool, cmdline: &str) -> Result<()> {
    let mut t = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ALL_ACCESS, &mut t)? };
    let user_buf = token_info(t, TokenUser)?;
    let user = unsafe { &*(user_buf.as_ptr() as *const TOKEN_USER) };
    let logon = logon_sid()?;
    eprintln!("[spike] restricted level {level}; logon sid {logon}");
    let logon_psid = string_sid(&logon)?;
    let everyone = string_sid("S-1-1-0")?;
    let users = string_sid("S-1-5-32-545")?;
    let restricted = string_sid("S-1-5-12")?;
    let admins = string_sid("S-1-5-32-544")?;
    let mut disable: Vec<SID_AND_ATTRIBUTES> = vec![SID_AND_ATTRIBUTES { Sid: admins, Attributes: 0 }];
    let mut restrict: Vec<SID_AND_ATTRIBUTES> = Vec::new();
    match level {
        // Low integrity only, privileges dropped.
        "low" => {}
        // + restricting SIDs: an object must grant one of these too.
        "limited" => restrict = [everyone, users, restricted, logon_psid].into_iter().map(|s| SID_AND_ATTRIBUTES { Sid: s, Attributes: 0 }).collect(),
        // + the user's own SID made deny-only: what only the user may read is closed.
        "lockdown" => {
            disable.push(SID_AND_ATTRIBUTES { Sid: user.User.Sid, Attributes: 0 });
            restrict = [everyone, users, restricted, logon_psid].into_iter().map(|s| SID_AND_ATTRIBUTES { Sid: s, Attributes: 0 }).collect();
        }
        other => bail!("unknown level {other}"),
    }
    for g in grants {
        // Low processes may write only to Low-labelled paths; grant the logon SID, which the token keeps.
        for args in [vec![g.as_str(), "/setintegritylevel", "(OI)(CI)L", "/q"], vec![g.as_str(), "/grant", &format!("*{logon}:(OI)(CI)(F)"), "/q"]] {
            let st = std::process::Command::new("icacls").args(&args).status()?;
            if !st.success() { bail!("icacls failed: {args:?}"); }
        }
    }
    let mut newt = HANDLE::default();
    unsafe {
        CreateRestrictedToken(
            t,
            DISABLE_MAX_PRIVILEGE,
            Some(&disable),
            None,
            if restrict.is_empty() { None } else { Some(&restrict) },
            &mut newt,
        )?
    };
    // Low integrity
    let low = string_sid("S-1-16-4096")?;
    let label = TOKEN_MANDATORY_LABEL { Label: SID_AND_ATTRIBUTES { Sid: low, Attributes: 0x20 /*SE_GROUP_INTEGRITY*/ } };
    unsafe { SetTokenInformation(newt, TokenIntegrityLevel, &label as *const _ as *const c_void, std::mem::size_of::<TOKEN_MANDATORY_LABEL>() as u32)? };
    let si = STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, ..Default::default() };
    let mut pi = PROCESS_INFORMATION::default();
    let mut cmd_w = wide(cmdline);
    let cwd_w = cwd.map(wide);
    let started = std::time::Instant::now();
    unsafe {
        CreateProcessAsUserW(
            Some(newt),
            PCWSTR::null(),
            Some(PWSTR(cmd_w.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP,
            None,
            cwd_w.as_ref().map(|w| PCWSTR(w.as_ptr())).unwrap_or(PCWSTR::null()),
            &si,
            &mut pi,
        )
        .context("CreateProcessAsUserW")?
    };
    let mut hjob = HANDLE::default();
    if job {
        hjob = unsafe { CreateJobObjectW(None, PCWSTR::null())? };
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        unsafe {
            SetInformationJobObject(hjob, JobObjectExtendedLimitInformation, &info as *const _ as *const c_void, std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32)?;
            AssignProcessToJobObject(hjob, pi.hProcess)?;
        }
    }
    unsafe { ResumeThread(pi.hThread) };
    let w = unsafe { WaitForSingleObject(pi.hProcess, INFINITE) };
    if w != WAIT_OBJECT_0 { bail!("wait failed"); }
    let mut code = 0u32;
    unsafe { GetExitCodeProcess(pi.hProcess, &mut code)? };
    eprintln!("[spike] exit {code} after {:?}", started.elapsed());
    unsafe { let _ = CloseHandle(pi.hThread); let _ = CloseHandle(pi.hProcess); if job { let _ = CloseHandle(hjob); } }
    std::process::exit(code as i32);
}

// ---------- named pipe with an owner-only DACL ----------
fn pipe_server(name: &str, open: bool) -> Result<()> {
    use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_TYPE_BYTE, PIPE_READMODE_BYTE, PIPE_WAIT, PIPE_REJECT_REMOTE_CLIENTS, GetNamedPipeClientProcessId};
    use windows::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
    use windows::Win32::Security::{SECURITY_ATTRIBUTES, PSECURITY_DESCRIPTOR};
    use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
    let mut t = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ACCESS_MASK(0x0008), &mut t)? };
    let user_buf = token_info(t, TokenUser)?;
    let user = unsafe { &*(user_buf.as_ptr() as *const TOKEN_USER) };
    let me = sid_string(user.User.Sid)?;
    // Only this user (as a normal, medium-or-higher process): no AppContainer, no restricted token, nobody else.
    let sddl = if open { wide(&format!("O:{me}D:P(A;;GA;;;WD)(A;;GA;;;AC)S:(ML;;NW;;;LW)")) } else { wide(&format!("O:{me}D:P(A;;GA;;;{me})S:(ML;;NW;;;ME)")) };
    let mut sd = PSECURITY_DESCRIPTOR::default();
    unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(sddl.as_ptr()), 1, &mut sd, None)? };
    let sa = SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: sd.0, bInheritHandle: false.into() };
    let path = wide(&format!("\\\\.\\pipe\\{name}"));
    let h = unsafe { CreateNamedPipeW(PCWSTR(path.as_ptr()), PIPE_ACCESS_DUPLEX, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS, 4, 4096, 4096, 0, Some(&sa)) };
    if h.is_invalid() { bail!("CreateNamedPipe failed: {:?}", windows::core::Error::from_thread()); }
    println!("serving \\\\.\\pipe\\{name} for 3 clients or 40 s, owner {me}, open={open}");
    let end = std::time::Instant::now() + std::time::Duration::from_secs(40);
    let mut served = 0;
    while std::time::Instant::now() < end && served < 3 {
        // ConnectNamedPipe blocks; acceptable for the spike.
        if unsafe { ConnectNamedPipe(h, None) }.is_err() { if windows::core::Error::from_thread().code().0 as u32 != 0x80070217 { continue; } }
        let mut pid = 0u32; let _ = unsafe { GetNamedPipeClientProcessId(h, &mut pid) };
        let mut buf = [0u8; 256]; let mut n = 0u32;
        let _ = unsafe { ReadFile(h, Some(&mut buf), Some(&mut n), None) };
        println!("client pid {pid} said {:?}", String::from_utf8_lossy(&buf[..n as usize]));
        let _ = unsafe { WriteFile(h, Some(b"hello"), None, None) };
        let _ = unsafe { DisconnectNamedPipe(h) };
        served += 1;
    }
    Ok(())
}

fn pipe_client(name: &str) -> Result<()> {
    use std::io::{Read, Write};
    let path = format!("\\\\.\\pipe\\{name}");
    match std::fs::OpenOptions::new().read(true).write(true).open(&path) {
        Ok(mut f) => {
            f.write_all(b"hi from client")?;
            let mut buf = [0u8; 16];
            let n = f.read(&mut buf).unwrap_or(0);
            println!("connected, server said {:?}", String::from_utf8_lossy(&buf[..n]));
        }
        Err(e) => { println!("refused: {e}"); std::process::exit(2); }
    }
    Ok(())
}

// ---------- block clone (FSCTL_DUPLICATE_EXTENTS_TO_FILE) ----------
fn clone_tree(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_READ, OPEN_EXISTING, CREATE_NEW, SetFileInformationByHandle, FileEndOfFileInfo, FILE_END_OF_FILE_INFO, GetDiskFreeSpaceW};
    use windows::Win32::System::Ioctl::{FSCTL_DUPLICATE_EXTENTS_TO_FILE, DUPLICATE_EXTENTS_DATA};
    use windows::Win32::System::IO::DeviceIoControl;
    let started = std::time::Instant::now();
    // cluster size of the destination volume
    let root = wide(&format!("{}\\", dst.components().next().map(|c| c.as_os_str().to_string_lossy().to_string()).unwrap_or_default()));
    let (mut spc, mut bps, mut a, mut b) = (0u32, 0u32, 0u32, 0u32);
    unsafe { GetDiskFreeSpaceW(PCWSTR(root.as_ptr()), Some(&mut spc), Some(&mut bps), Some(&mut a), Some(&mut b))? };
    let cluster = (spc * bps) as u64;
    let (mut files, mut bytes, mut cloned, mut copied) = (0u64, 0u64, 0u64, 0u64);
    let mut stack = vec![src.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rel = dir.strip_prefix(src).unwrap();
        std::fs::create_dir_all(dst.join(rel))?;
        for e in std::fs::read_dir(&dir)? {
            let e = e?;
            let p = e.path();
            if e.file_type()?.is_dir() { stack.push(p); continue; }
            let size = e.metadata()?.len();
            let to = dst.join(p.strip_prefix(src).unwrap());
            files += 1; bytes += size;
            if size == 0 { std::fs::File::create(&to)?; continue; }
            let sw = wide(p.to_str().unwrap()); let dw = wide(to.to_str().unwrap());
            let hs = unsafe { CreateFileW(PCWSTR(sw.as_ptr()), FILE_GENERIC_READ.0, FILE_SHARE_READ, None, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, None)? };
            let hd = unsafe { CreateFileW(PCWSTR(dw.as_ptr()), FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0, FILE_SHARE_READ, None, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, None)? };
            let eof = FILE_END_OF_FILE_INFO { EndOfFile: size as i64 };
            unsafe { SetFileInformationByHandle(hd, FileEndOfFileInfo, &eof as *const _ as *const c_void, std::mem::size_of::<FILE_END_OF_FILE_INFO>() as u32)? };
            let rounded = size.div_ceil(cluster) * cluster;
            let data = DUPLICATE_EXTENTS_DATA { FileHandle: hs, SourceFileOffset: 0, TargetFileOffset: 0, ByteCount: rounded as i64 };
            let r = unsafe { DeviceIoControl(hd, FSCTL_DUPLICATE_EXTENTS_TO_FILE, Some(&data as *const _ as *const c_void), std::mem::size_of::<DUPLICATE_EXTENTS_DATA>() as u32, None, 0, None, None) };
            unsafe { let _ = CloseHandle(hs); let _ = CloseHandle(hd); }
            match r {
                Ok(()) => cloned += 1,
                Err(e) => {
                    if copied == 0 { eprintln!("[clone] first fallback: {e}"); }
                    std::fs::remove_file(&to)?; std::fs::copy(&p, &to)?; copied += 1;
                }
            }
        }
    }
    println!("{files} files, {} MB: {cloned} cloned, {copied} copied, cluster {cluster}, {:.2?}", bytes / 1_048_576, started.elapsed());
    Ok(())
}
