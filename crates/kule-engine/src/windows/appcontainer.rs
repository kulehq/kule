//! The app container Kule's proxy runs in on Windows. An app container is
//! a process whose token names a package rather than the person: it reads
//! only what is open to every package or granted to its own, holds none
//! of the person's credentials, and reaches the network only through the
//! capabilities it is given. Kule's proxy gets a profile of its own,
//! `Kule.Proxy`, made per user on first use (no administrator needed), the
//! two capabilities that let it connect out, a pipe each way to Kule, and
//! nothing else: not the task's files, not Kule's home, not the handles
//! Kule holds. Not even its own program file is granted to it: Kule opens
//! the image, so a program under the person's own folders runs in the
//! container without its access list changing. A job with kill-on-close
//! around it ends it when Kule goes.
//!
//! Measured before it was built (`docs/windows-measurements.md`, M3): a
//! container that names this profile as its proxy peer reaches a listener
//! in it with the firewall at its defaults and no rule written, as an
//! administrator and as a standard user alike; a listener outside the
//! profile is not reached, and neither is one inside it from a plain
//! process of the person's. So the proxy's port is a dead end for
//! everything but the task's sandbox.

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::time::Duration;

use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, HANDLE, HANDLE_FLAG_INHERIT, HANDLE_FLAGS, LocalFree,
    SetHandleInformation, WAIT_OBJECT_0,
};
use windows::Win32::Security::Authorization::ConvertStringSidToSidW;
use windows::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
};
use windows::Win32::Security::{
    FreeSid, PSID, SECURITY_ATTRIBUTES, SECURITY_CAPABILITIES, SID_AND_ATTRIBUTES,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::SystemServices::SE_GROUP_ENABLED;
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW,
    DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess,
    InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
    PROCESS_INFORMATION, ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess,
    UpdateProcThreadAttribute, WaitForSingleObject,
};
use windows::core::{PCWSTR, PWSTR};

use crate::osfs::win::{io_error, sid_string};
use crate::proxy_pipe::{ChildProcess, Launched};

/// An error of one Windows call, named: which call failed is what a
/// person needs when a container cannot be made.
fn at(step: &'static str) -> impl Fn(windows::core::Error) -> io::Error {
    move |e| {
        let e = io_error(e);
        io::Error::new(e.kind(), format!("{step}: {e}"))
    }
}

/// The profile the proxy runs under. Its SID is derived from this name
/// alone, the same on every computer.
pub const PROFILE: &str = "Kule.Proxy";

/// The capabilities the proxy gets: to connect to the internet, and to
/// the local network for what a policy lets a command reach by name.
const CAPABILITIES: &[&str] = &["S-1-15-3-1", "S-1-15-3-3"];

/// How long making a profile may take, tries and its folder included,
/// and how long between tries.
const PROFILE_WAIT: Duration = Duration::from_secs(20);
const PROFILE_RETRY: Duration = Duration::from_millis(100);

/// A lock across this user's processes around making one profile: a named
/// mutex of the session, held until dropped.
struct MakerLock(OwnedHandle);

impl MakerLock {
    fn take(name: &str) -> io::Result<MakerLock> {
        use windows::Win32::Foundation::WAIT_ABANDONED;
        use windows::Win32::System::Threading::CreateMutexW;
        let mutex_name = wide(OsStr::new(&format!("Local\\{name}.making")));
        // SAFETY: the name is NUL-terminated; the handle is owned below and
        // the mutex released in Drop.
        unsafe {
            let mutex = CreateMutexW(None, false, PCWSTR(mutex_name.as_ptr()))
                .map_err(at("CreateMutexW"))?;
            let mutex = OwnedHandle::from_raw_handle(mutex.0);
            let ms = u32::try_from(PROFILE_WAIT.as_millis()).unwrap_or(u32::MAX);
            match WaitForSingleObject(handle(&mutex), ms) {
                // Abandoned by a process that died holding it: ours now.
                w if w == WAIT_OBJECT_0 || w == WAIT_ABANDONED => Ok(MakerLock(mutex)),
                _ => Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("another process has been making the profile {name} for too long"),
                )),
            }
        }
    }
}

impl Drop for MakerLock {
    fn drop(&mut self) {
        use windows::Win32::System::Threading::ReleaseMutex;
        // SAFETY: held since take.
        unsafe {
            let _ = ReleaseMutex(handle(&self.0));
        }
    }
}

/// Where the `kule-proxy` program is: beside this program, and nowhere
/// else, except that a test program cargo put in a `deps` folder looks in
/// the folder above it, where cargo puts the programs it built.
pub fn proxy_exe() -> io::Result<PathBuf> {
    let me = std::env::current_exe()?;
    let name = "kule-proxy.exe";
    let dir = me.parent();
    let beside = dir.map(|dir| dir.join(name));
    let above = dir
        .filter(|dir| dir.file_name().is_some_and(|n| n == "deps"))
        .and_then(Path::parent)
        .map(|dir| dir.join(name));
    [beside, above]
        .into_iter()
        .flatten()
        .find(|p| p.is_file())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("{name} is not beside {}", me.display()),
            )
        })
}

/// Starts `kule-proxy` in the proxy's container, with its pipes.
pub(crate) fn launch_proxy() -> io::Result<Launched> {
    let exe = proxy_exe()?;
    launch(&exe, &[])
}

/// Starts `exe` with `args` in the proxy's container, with a pipe each
/// way and one for its errors.
pub(crate) fn launch(exe: &Path, args: &[&OsStr]) -> io::Result<Launched> {
    launch_in(PROFILE, exe, args)
}

/// [`launch`] in the container of the profile `name`.
fn launch_in(name: &str, exe: &Path, args: &[&OsStr]) -> io::Result<Launched> {
    let profile = Profile::ensure(name)?;
    let (stdin_read, stdin_write) = pipe()?;
    let (stdout_read, stdout_write) = pipe()?;
    let (stderr_read, stderr_write) = pipe()?;
    // Kule's ends stay Kule's.
    for ours in [&stdin_write, &stdout_read, &stderr_read] {
        // SAFETY: an open handle this function owns.
        unsafe { SetHandleInformation(handle(ours), HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0)) }
            .map_err(at("SetHandleInformation"))?;
    }
    let process = profile.spawn(exe, args, [&stdin_read, &stdout_write, &stderr_write])?;
    drop((stdin_read, stdout_write, stderr_write));
    Ok(Launched {
        stdin: Box::new(std::fs::File::from(stdin_write)),
        stdout: Box::new(std::fs::File::from(stdout_read)),
        stderr: Some(Box::new(std::fs::File::from(stderr_read))),
        process: Box::new(process),
    })
}

/// A program running in the proxy's container, which ends with this: for
/// the tests that show the container's processes end with Kule's.
pub struct Running {
    pid: u32,
    _process: Box<dyn ChildProcess>,
}

impl Running {
    pub fn pid(&self) -> u32 {
        self.pid
    }
}

/// Starts `exe` with `args` in the proxy's container, with no pipes to
/// speak of, and leaves it running.
pub fn start(exe: &Path, args: &[&OsStr]) -> io::Result<Running> {
    let launched = launch(exe, args)?;
    Ok(Running {
        pid: launched.process.pid(),
        _process: launched.process,
    })
}

/// Runs `exe` with `args` in the proxy's container and waits up to
/// `timeout` for it: its exit code (`None` when it did not end in time)
/// and what it printed as errors. For the tests that show what the
/// container can and cannot reach.
pub fn run(exe: &Path, args: &[&OsStr], timeout: Duration) -> io::Result<(Option<u32>, String)> {
    run_in(PROFILE, exe, args, timeout)
}

/// [`run`] in the container of the profile `name`, made when it is not
/// there: for the tests of making one.
#[doc(hidden)]
pub fn run_in(
    name: &str,
    exe: &Path,
    args: &[&OsStr],
    timeout: Duration,
) -> io::Result<(Option<u32>, String)> {
    use std::io::Read;
    let Launched {
        stdin,
        stdout,
        stderr,
        mut process,
    } = launch_in(name, exe, args)?;
    drop(stdin);
    let pid = process.pid();
    let drain = |mut from: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = from.read_to_string(&mut text);
            text
        })
    };
    let out = drain(stdout);
    let err = stderr.map(drain);
    let ended = process.wait_for(timeout);
    if !ended {
        process.kill();
    }
    let _ = out.join();
    let errors = err.and_then(|t| t.join().ok()).unwrap_or_default();
    let code = ended.then(|| exit_code_of(pid)).flatten();
    Ok((code, errors))
}

/// Removes the profile `name` of this user, for the tests of making one;
/// one that is not there is nothing to remove.
#[doc(hidden)]
pub fn remove_profile(name: &str) -> io::Result<()> {
    use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows::Win32::Security::Isolation::DeleteAppContainerProfile;
    let wide_name = wide(OsStr::new(name));
    // SAFETY: the string is NUL-terminated and outlives the call.
    match unsafe { DeleteAppContainerProfile(PCWSTR(wide_name.as_ptr())) } {
        Ok(()) => Ok(()),
        Err(e) if e.code() == ERROR_FILE_NOT_FOUND.to_hresult() => Ok(()),
        Err(e) => Err(at("DeleteAppContainerProfile")(e)),
    }
}

/// The proxy's profile: its SID, owned.
struct Profile {
    sid: PSID,
}

impl Profile {
    /// The profile `name`, made for this user when it is not there yet.
    /// One maker at a time, across Kule's processes: two making the same
    /// profile at once (the daemon and a command of the first run, say)
    /// trip over each other, one told the environment is wrong, the other
    /// given a profile no process can start in. A maker that still meets
    /// another (an older Kule, say) tries again, and nothing is used before
    /// its folder is there.
    fn ensure(name: &str) -> io::Result<Profile> {
        static MAKING: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _making = MAKING.lock().unwrap_or_else(|e| e.into_inner());
        let _across = MakerLock::take(name)?;
        let wide_name = wide(OsStr::new(name));
        let shown = wide(OsStr::new("Kule proxy"));
        let deadline = std::time::Instant::now() + PROFILE_WAIT;
        let sid = loop {
            // SAFETY: the strings are NUL-terminated and outlive the calls;
            // the SID returned is freed in Drop.
            let made = unsafe {
                match CreateAppContainerProfile(
                    PCWSTR(wide_name.as_ptr()),
                    PCWSTR(shown.as_ptr()),
                    PCWSTR(shown.as_ptr()),
                    None,
                ) {
                    Ok(sid) => Ok(sid),
                    Err(e) if e.code() == ERROR_ALREADY_EXISTS.to_hresult() => {
                        DeriveAppContainerSidFromAppContainerName(PCWSTR(wide_name.as_ptr()))
                            .map_err(at("DeriveAppContainerSidFromAppContainerName"))
                    }
                    Err(e) => Err(at("CreateAppContainerProfile")(e)),
                }
            };
            match made {
                Ok(sid) => break sid,
                Err(e) if std::time::Instant::now() < deadline => {
                    let _ = e;
                    std::thread::sleep(PROFILE_RETRY);
                }
                Err(e) => return Err(e),
            }
        };
        let profile = Profile { sid };
        profile.wait_for_folder(deadline)?;
        Ok(profile)
    }

    /// Waits until the profile's folder is there, which is when a process
    /// can be started in it: a profile another maker has half made has no
    /// folder yet, or none Windows can name. One whose folder never comes
    /// is told of.
    fn wait_for_folder(&self, deadline: std::time::Instant) -> io::Result<()> {
        loop {
            let missing = match self.folder() {
                Ok(folder) if folder.is_dir() => return Ok(()),
                Ok(folder) => io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("the container's folder {} is not there", folder.display()),
                ),
                Err(e) => e,
            };
            if std::time::Instant::now() >= deadline {
                return Err(missing);
            }
            std::thread::sleep(PROFILE_RETRY);
        }
    }

    /// The folder Windows keeps for the profile.
    fn folder(&self) -> io::Result<PathBuf> {
        use windows::Win32::Security::Isolation::GetAppContainerFolderPath;
        use windows::Win32::System::Com::CoTaskMemFree;
        // SAFETY: a SID the profile calls returned; the path string is
        // freed here once copied.
        unsafe {
            let text = sid_string(self.sid)?;
            let wide_text = wide(OsStr::new(&text));
            let path = GetAppContainerFolderPath(PCWSTR(wide_text.as_ptr()))
                .map_err(at("GetAppContainerFolderPath"))?;
            let folder = PathBuf::from(path.to_string().unwrap_or_default());
            CoTaskMemFree(Some(path.0.cast()));
            Ok(folder)
        }
    }

    /// Starts `exe` with `args` in the container, `stdio` as its standard
    /// handles and the only handles it inherits, inside a job that ends it
    /// when the job's handle closes.
    fn spawn(
        &self,
        exe: &Path,
        args: &[&OsStr],
        stdio: [&OwnedHandle; 3],
    ) -> io::Result<Contained> {
        let mut capabilities = Vec::new();
        for text in CAPABILITIES {
            let wide_text = wide(OsStr::new(text));
            let mut sid = PSID::default();
            // SAFETY: the string is NUL-terminated; the SID is freed below.
            unsafe { ConvertStringSidToSidW(PCWSTR(wide_text.as_ptr()), &mut sid) }
                .map_err(at("ConvertStringSidToSidW"))?;
            capabilities.push(sid);
        }
        let result = self.spawn_with(exe, args, stdio, &capabilities);
        for sid in capabilities {
            // SAFETY: allocated by ConvertStringSidToSidW.
            unsafe {
                let _ = LocalFree(Some(windows::Win32::Foundation::HLOCAL(sid.0)));
            }
        }
        result
    }

    fn spawn_with(
        &self,
        exe: &Path,
        args: &[&OsStr],
        stdio: [&OwnedHandle; 3],
        capabilities: &[PSID],
    ) -> io::Result<Contained> {
        let mut attributes: Vec<SID_AND_ATTRIBUTES> = capabilities
            .iter()
            .map(|sid| SID_AND_ATTRIBUTES {
                Sid: *sid,
                Attributes: SE_GROUP_ENABLED as u32,
            })
            .collect();
        let mut security = SECURITY_CAPABILITIES {
            AppContainerSid: self.sid,
            Capabilities: attributes.as_mut_ptr(),
            CapabilityCount: attributes.len() as u32,
            Reserved: 0,
        };
        let handles: [HANDLE; 3] = [handle(stdio[0]), handle(stdio[1]), handle(stdio[2])];
        let list = AttributeList::new(2)?;
        // SAFETY: the values outlive the list, which outlives the process
        // creation below.
        unsafe {
            UpdateProcThreadAttribute(
                list.0,
                0,
                PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
                Some((&mut security as *mut SECURITY_CAPABILITIES).cast()),
                std::mem::size_of::<SECURITY_CAPABILITIES>(),
                None,
                None,
            )
            .map_err(at("UpdateProcThreadAttribute (capabilities)"))?;
            UpdateProcThreadAttribute(
                list.0,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                Some(handles.as_ptr().cast()),
                std::mem::size_of::<HANDLE>() * handles.len(),
                None,
                None,
            )
            .map_err(at("UpdateProcThreadAttribute (handles)"))?;
        }
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = handles[0];
        startup.StartupInfo.hStdOutput = handles[1];
        startup.StartupInfo.hStdError = handles[2];
        startup.lpAttributeList = list.0;
        let mut command_line = wide(command_line(exe, args).as_os_str());
        let exe_wide = wide(exe.as_os_str());
        let environment = environment();
        let cwd = wide(system_directory().as_os_str());
        let mut info = PROCESS_INFORMATION::default();
        // SAFETY: every pointer is to a buffer that outlives the call; the
        // handles returned are owned below.
        unsafe {
            CreateProcessW(
                PCWSTR(exe_wide.as_ptr()),
                Some(PWSTR(command_line.as_mut_ptr())),
                None,
                None,
                true,
                EXTENDED_STARTUPINFO_PRESENT
                    | CREATE_SUSPENDED
                    | CREATE_NO_WINDOW
                    | CREATE_UNICODE_ENVIRONMENT,
                Some(environment.as_ptr().cast()),
                PCWSTR(cwd.as_ptr()),
                &startup.StartupInfo,
                &mut info,
            )
            .map_err(at("CreateProcessW"))?;
        }
        // SAFETY: handles CreateProcessW just returned, owned from here.
        let (process, thread) = unsafe {
            (
                OwnedHandle::from_raw_handle(info.hProcess.0),
                OwnedHandle::from_raw_handle(info.hThread.0),
            )
        };
        let contained = Contained {
            pid: info.dwProcessId,
            process,
            job: Job::around(info.hProcess)?,
        };
        // SAFETY: the thread handle is open; it is closed when dropped.
        unsafe {
            ResumeThread(handle(&thread));
        }
        drop(thread);
        drop(list);
        Ok(contained)
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        // SAFETY: a SID the profile calls allocated.
        unsafe {
            FreeSid(self.sid);
        }
    }
}

/// A job that ends every process in it when this handle closes.
struct Job(OwnedHandle);

impl Job {
    fn around(process: HANDLE) -> io::Result<Job> {
        // SAFETY: the job handle is owned below; the process handle is
        // open; the limit struct is the size the class wants.
        unsafe {
            let job = CreateJobObjectW(None, PCWSTR::null()).map_err(at("CreateJobObjectW"))?;
            let job = Job(OwnedHandle::from_raw_handle(job.0));
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                handle(&job.0),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .map_err(at("SetInformationJobObject"))?;
            AssignProcessToJobObject(handle(&job.0), process)
                .map_err(at("AssignProcessToJobObject"))?;
            Ok(job)
        }
    }
}

/// A process in the container, ended with its job.
struct Contained {
    pid: u32,
    process: OwnedHandle,
    #[allow(dead_code)]
    job: Job,
}

/// The exit code of a process that has ended, by its id, while a handle
/// to it is still open somewhere.
fn exit_code_of(pid: u32) -> Option<u32> {
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: the handle is owned and closed when dropped.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let process = OwnedHandle::from_raw_handle(process.0);
        let mut code = 0u32;
        GetExitCodeProcess(handle(&process), &mut code)
            .ok()
            .map(|()| code)
    }
}

impl ChildProcess for Contained {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn wait_for(&mut self, timeout: Duration) -> bool {
        let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: the process handle is open.
        unsafe { WaitForSingleObject(handle(&self.process), ms) == WAIT_OBJECT_0 }
    }

    fn kill(&mut self) {
        // SAFETY: the process handle is open; a process already gone is an
        // error that means nothing here.
        unsafe {
            let _ = TerminateProcess(handle(&self.process), 1);
            let _ = WaitForSingleObject(handle(&self.process), 5_000);
        }
    }
}

/// A process-thread attribute list with room for `count` attributes, in
/// the buffer it lives in.
struct AttributeList(LPPROC_THREAD_ATTRIBUTE_LIST, #[allow(dead_code)] Vec<u8>);

impl AttributeList {
    fn new(count: u32) -> io::Result<AttributeList> {
        let mut size = 0usize;
        // SAFETY: asked for the size first, then given a buffer that large.
        unsafe {
            let _ = InitializeProcThreadAttributeList(None, count, None, &mut size);
            let mut buffer = vec![0u8; size];
            let list = LPPROC_THREAD_ATTRIBUTE_LIST(buffer.as_mut_ptr().cast());
            InitializeProcThreadAttributeList(Some(list), count, None, &mut size)
                .map_err(at("InitializeProcThreadAttributeList"))?;
            Ok(AttributeList(list, buffer))
        }
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        // SAFETY: initialized in new; the buffer outlives this call.
        unsafe { DeleteProcThreadAttributeList(self.0) };
    }
}

/// An inheritable pipe: its read and write ends.
fn pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: true.into(),
    };
    let (mut read, mut write) = (HANDLE::default(), HANDLE::default());
    // SAFETY: the handles are owned once the call returns them.
    unsafe {
        CreatePipe(&mut read, &mut write, Some(&attributes), 0).map_err(at("CreatePipe"))?;
        Ok((
            OwnedHandle::from_raw_handle(read.0),
            OwnedHandle::from_raw_handle(write.0),
        ))
    }
}

fn handle(owned: &OwnedHandle) -> HANDLE {
    HANDLE(owned.as_raw_handle())
}

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// `exe` and `args` as one command line, each quoted as the C runtime
/// reads it back.
fn command_line(exe: &Path, args: &[&OsStr]) -> std::ffi::OsString {
    let mut line = std::ffi::OsString::new();
    line.push(quoted(exe.as_os_str()));
    for arg in args {
        line.push(" ");
        line.push(quoted(arg));
    }
    line
}

/// `arg` as a command-line argument the C runtime reads back as itself:
/// quoted where it holds a space, a tab, a quote or nothing, and left as
/// it is otherwise (`cmd.exe` takes its own switches unquoted only).
pub(crate) fn quoted(arg: &OsStr) -> std::ffi::OsString {
    let text = arg.to_string_lossy();
    if !text.is_empty() && !text.contains([' ', '\t', '"', '\n']) {
        return arg.to_owned();
    }
    let mut out = std::ffi::OsString::from("\"");
    let mut backslashes = 0;
    for c in text.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push("\\".repeat(backslashes * 2 + 1));
                out.push("\"");
                backslashes = 0;
            }
            other => {
                out.push("\\".repeat(backslashes));
                out.push(other.to_string());
                backslashes = 0;
            }
        }
    }
    out.push("\\".repeat(backslashes * 2));
    out.push("\"");
    out
}

/// What the process gets as its environment: where Windows is, the
/// system folder on its path, and the person's local data folder, which
/// Windows asks for to start a container and then points at the
/// container's own folder. Nothing else of Kule's own environment, which
/// may hold what an agent is run with.
fn environment() -> Vec<u16> {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let drive = std::env::var_os("SystemDrive").unwrap_or_else(|| "C:".into());
    let local = std::env::var_os("LOCALAPPDATA").unwrap_or_default();
    let mut block = Vec::new();
    for (name, value) in [
        ("SystemRoot", root.clone()),
        ("SystemDrive", drive),
        ("windir", root.clone()),
        ("PATH", Path::new(&root).join("System32").into_os_string()),
        ("LOCALAPPDATA", local),
    ] {
        block.extend(OsStr::new(name).encode_wide());
        block.push(u16::from(b'='));
        block.extend(value.encode_wide());
        block.push(0);
    }
    block.push(0);
    block
}

/// A folder every package may stand in.
fn system_directory() -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    Path::new(&root).join("System32")
}
