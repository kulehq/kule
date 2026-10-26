//! The connection between the daemon and its clients: a Unix socket in
//! Kule's home on macOS and Linux, a named pipe on Windows. Either way only
//! the same user's own processes get through: the other end is asked who
//! it is before a token is sent or a request is read, and on Windows a
//! process in an app container is refused as not the person's own.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where the daemon for `home` listens: a socket file in the home, or a
/// pipe named after the home.
pub fn endpoint(home: &Path) -> PathBuf {
    #[cfg(unix)]
    {
        home.join("kuled.sock")
    }
    #[cfg(windows)]
    {
        let canonical = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
        let text = canonical.to_string_lossy().to_lowercase();
        let hash = blake3::hash(text.as_bytes()).to_hex();
        PathBuf::from(format!(r"\\.\pipe\kule-{}", &hash[..16]))
    }
}

/// One connection, either side of it.
pub struct Stream {
    #[cfg(unix)]
    inner: std::os::unix::net::UnixStream,
    #[cfg(windows)]
    inner: pipes::Pipe,
}

/// What accepts connections.
pub struct Listener {
    #[cfg(unix)]
    inner: std::os::unix::net::UnixListener,
    #[cfg(windows)]
    inner: pipes::Server,
}

impl Stream {
    /// Connects to the daemon at `at`.
    pub fn connect(at: &Path) -> io::Result<Stream> {
        #[cfg(unix)]
        {
            std::os::unix::net::UnixStream::connect(at).map(|inner| Stream { inner })
        }
        #[cfg(windows)]
        {
            pipes::Pipe::connect(at).map(|inner| Stream { inner })
        }
    }

    /// Whether the process on the other end runs as this user, and not
    /// inside an app container: what a request may be read from, and the
    /// token sent to.
    pub fn peer_is_me(&self) -> bool {
        #[cfg(unix)]
        {
            // SAFETY: geteuid has no preconditions and cannot fail.
            let me = unsafe { libc::geteuid() };
            peer_uid(&self.inner) == Some(me)
        }
        #[cfg(windows)]
        {
            self.inner.peer_is_me()
        }
    }

    pub fn try_clone(&self) -> io::Result<Stream> {
        self.inner.try_clone().map(|inner| Stream { inner })
    }

    /// Reads from now on give up after `timeout`; `None` waits.
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.inner.set_read_timeout(timeout)
    }

    /// Ends the connection for both sides.
    pub fn shutdown(&self) -> io::Result<()> {
        #[cfg(unix)]
        {
            self.inner.shutdown(std::net::Shutdown::Both)
        }
        #[cfg(windows)]
        {
            self.inner.shutdown()
        }
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl Listener {
    /// Listens at `at`, which must be free: a stale socket file is replaced
    /// (Unix); a pipe name already taken is refused (Windows).
    pub fn bind(at: &Path) -> io::Result<Listener> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if at.exists() {
                if std::os::unix::net::UnixStream::connect(at).is_ok() {
                    return Err(io::Error::new(
                        io::ErrorKind::AddrInUse,
                        "a Kule daemon is already running",
                    ));
                }
                std::fs::remove_file(at)?;
            }
            let inner = std::os::unix::net::UnixListener::bind(at)?;
            std::fs::set_permissions(at, std::fs::Permissions::from_mode(0o600))?;
            Ok(Listener { inner })
        }
        #[cfg(windows)]
        {
            pipes::Server::bind(at).map(|inner| Listener { inner })
        }
    }

    /// The next connection.
    pub fn accept(&self) -> io::Result<Stream> {
        #[cfg(unix)]
        {
            self.inner.accept().map(|(inner, _)| Stream { inner })
        }
        #[cfg(windows)]
        {
            self.inner.accept().map(|inner| Stream { inner })
        }
    }

    /// Removes what listening left behind, once the daemon stops.
    pub fn remove(at: &Path) {
        #[cfg(unix)]
        let _ = std::fs::remove_file(at);
        #[cfg(windows)]
        let _ = at;
    }
}

/// The uid of the process on the other end of a Unix socket.
#[cfg(unix)]
pub fn peer_uid(stream: &std::os::unix::net::UnixStream) -> Option<u32> {
    use std::os::fd::AsRawFd;
    let fd = stream.as_raw_fd();
    #[cfg(target_os = "macos")]
    {
        let (mut uid, mut gid): (libc::uid_t, libc::gid_t) = (0, 0);
        // SAFETY: fd is a valid socket for the lifetime of `stream`; the out
        // pointers point to live stack variables.
        let rc = unsafe { libc::getpeereid(fd, &mut uid, &mut gid) };
        (rc == 0).then_some(uid)
    }
    #[cfg(target_os = "linux")]
    {
        let mut cred = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: fd is a valid socket; cred and len are live and sized right.
        let rc = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut cred as *mut _ as *mut libc::c_void,
                &mut len,
            )
        };
        (rc == 0).then_some(cred.uid)
    }
}

/// Named pipes, in overlapped mode so a read can give up on time: each
/// instance carries an access list naming only this user and the system,
/// refuses remote clients, and the first instance's name must be free.
#[cfg(windows)]
mod pipes {
    use std::io::{self, Read, Write};
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::path::Path;
    use std::sync::Mutex;
    use std::time::Duration;

    use kule_engine::osfs::win::{current_user_sid, io_error, is_app_container, user_sid_of};
    use windows::Win32::Foundation::{
        CloseHandle, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, HANDLE, WAIT_TIMEOUT,
    };
    use windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX, ReadFile,
        WriteFile,
    };
    use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
        GetNamedPipeServerProcessId, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
        PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
    use windows::core::PCWSTR;

    const BUFFER: u32 = 64 * 1024;

    fn wide(path: &Path) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// One end of a pipe, as the daemon or a client holds it.
    pub struct Pipe {
        handle: OwnedHandle,
        /// Whether this end is the server's instance, which disconnects
        /// instead of merely closing.
        server: bool,
        timeout: Mutex<Option<Duration>>,
    }

    pub struct Server {
        name: Vec<u16>,
        descriptor: PSECURITY_DESCRIPTOR,
        /// The instance waiting for the next client.
        next: Mutex<Option<OwnedHandle>>,
    }

    // SAFETY: the descriptor is only read, by every instance made.
    unsafe impl Send for Server {}
    unsafe impl Sync for Server {}

    impl Drop for Server {
        fn drop(&mut self) {
            // SAFETY: allocated by the conversion call in `bind`.
            unsafe {
                let _ = windows::Win32::Foundation::LocalFree(Some(
                    windows::Win32::Foundation::HLOCAL(self.descriptor.0),
                ));
            }
        }
    }

    impl Server {
        pub fn bind(at: &Path) -> io::Result<Server> {
            let sid = current_user_sid()?;
            let sddl = format!("D:P(A;;GA;;;{sid})(A;;GA;;;SY)");
            let sddl_wide: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            // SAFETY: the descriptor is allocated by the call and freed in Drop.
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    PCWSTR(sddl_wide.as_ptr()),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    None,
                )
            }
            .map_err(io_error)?;
            let server = Server {
                name: wide(at),
                descriptor,
                next: Mutex::new(None),
            };
            let first = server.instance(true).map_err(|e| {
                if e.raw_os_error() == Some(5) {
                    io::Error::new(
                        io::ErrorKind::AddrInUse,
                        "a Kule daemon is already running, or something else holds its pipe",
                    )
                } else {
                    e
                }
            })?;
            *server
                .next
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(first);
            Ok(server)
        }

        fn instance(&self, first: bool) -> io::Result<OwnedHandle> {
            let attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: self.descriptor.0,
                bInheritHandle: false.into(),
            };
            let mut mode = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
            if first {
                mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
            }
            // SAFETY: the name and attributes live for the call; the handle
            // is owned below.
            let handle = unsafe {
                CreateNamedPipeW(
                    PCWSTR(self.name.as_ptr()),
                    mode,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    PIPE_UNLIMITED_INSTANCES,
                    BUFFER,
                    BUFFER,
                    0,
                    Some(&attributes),
                )
            };
            if handle.is_invalid() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: a handle the call just made, owned by nothing else.
            Ok(unsafe { OwnedHandle::from_raw_handle(handle.0) })
        }

        /// Waits for a client on the ready instance, then readies the next.
        pub fn accept(&self) -> io::Result<Pipe> {
            let waiting = self
                .next
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            let waiting = match waiting {
                Some(handle) => handle,
                None => self.instance(false)?,
            };
            let event = Event::new()?;
            let mut overlapped = OVERLAPPED {
                hEvent: event.0,
                ..Default::default()
            };
            // SAFETY: the handle is open; the overlapped block and its event
            // live until the wait below returns.
            let connected =
                unsafe { ConnectNamedPipe(HANDLE(waiting.as_raw_handle()), Some(&mut overlapped)) };
            match connected {
                Ok(()) => {}
                Err(e) if e.code() == ERROR_PIPE_CONNECTED.to_hresult() => {}
                Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => {
                    // SAFETY: as above.
                    unsafe {
                        WaitForSingleObject(event.0, u32::MAX);
                        let mut moved = 0u32;
                        GetOverlappedResult(
                            HANDLE(waiting.as_raw_handle()),
                            &overlapped,
                            &mut moved,
                            false,
                        )
                        .map_err(io_error)?;
                    }
                }
                Err(e) => return Err(io_error(e)),
            }
            // The next instance right away: a client arriving in between
            // finds no pipe for an instant and tries again (see `connect`).
            let next = self.instance(false)?;
            *self
                .next
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(next);
            Ok(Pipe {
                handle: waiting,
                server: true,
                timeout: Mutex::new(None),
            })
        }
    }

    /// An event handle for one overlapped call.
    struct Event(HANDLE);

    impl Event {
        fn new() -> io::Result<Event> {
            // SAFETY: a manual-reset event, closed in Drop.
            let handle =
                unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.map_err(io_error)?;
            Ok(Event(handle))
        }
    }

    impl Drop for Event {
        fn drop(&mut self) {
            // SAFETY: the handle was made by CreateEventW and is closed once.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    impl Pipe {
        pub fn connect(at: &Path) -> io::Result<Pipe> {
            use std::os::windows::fs::OpenOptionsExt;
            use windows::Win32::System::Pipes::WaitNamedPipeW;
            const SECURITY_SQOS_PRESENT: u32 = 0x0010_0000;
            const SECURITY_IDENTIFICATION: u32 = 0x0001_0000;
            const ERROR_PIPE_BUSY: i32 = 231;
            let name = wide(at);
            let started = std::time::Instant::now();
            loop {
                // Identification only: the daemon cannot act as this process.
                match std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .custom_flags(
                        FILE_FLAG_OVERLAPPED.0 | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    )
                    .open(at)
                {
                    Ok(file) => {
                        return Ok(Pipe {
                            handle: OwnedHandle::from(file),
                            server: false,
                            timeout: Mutex::new(None),
                        });
                    }
                    // Every instance is taken for the moment (the daemon is
                    // between one client and the next): wait for a free one.
                    Err(e)
                        if e.raw_os_error() == Some(ERROR_PIPE_BUSY)
                            && started.elapsed() < Duration::from_secs(5) =>
                    {
                        // SAFETY: the name lives for the call.
                        let _ = unsafe { WaitNamedPipeW(PCWSTR(name.as_ptr()), 1000) };
                    }
                    // No instance at all for an instant, as the daemon makes
                    // the next: a short while, not a wait for a daemon that
                    // is not there.
                    Err(e)
                        if e.kind() == io::ErrorKind::NotFound
                            && started.elapsed() < Duration::from_millis(300) =>
                    {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Err(e) => return Err(e),
                }
            }
        }

        fn raw(&self) -> HANDLE {
            HANDLE(self.handle.as_raw_handle())
        }

        pub fn peer_is_me(&self) -> bool {
            let mut pid = 0u32;
            // SAFETY: the handle is open; pid is a live local.
            let asked = unsafe {
                if self.server {
                    GetNamedPipeClientProcessId(self.raw(), &mut pid)
                } else {
                    GetNamedPipeServerProcessId(self.raw(), &mut pid)
                }
            };
            if asked.is_err() {
                return false;
            }
            let (Ok(me), Ok(them)) = (current_user_sid(), user_sid_of(pid)) else {
                return false;
            };
            me == them && !is_app_container(pid).unwrap_or(true)
        }

        pub fn try_clone(&self) -> io::Result<Pipe> {
            Ok(Pipe {
                handle: self.handle.try_clone()?,
                server: self.server,
                timeout: Mutex::new(
                    *self
                        .timeout
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner),
                ),
            })
        }

        pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
            *self
                .timeout
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = timeout;
            Ok(())
        }

        pub fn shutdown(&self) -> io::Result<()> {
            // SAFETY: the handle is open; pending calls are cancelled first.
            unsafe {
                let _ = CancelIoEx(self.raw(), None);
                if self.server {
                    DisconnectNamedPipe(self.raw()).map_err(io_error)?;
                }
            }
            Ok(())
        }

        /// Waits for the overlapped call `overlapped` on `event`, for
        /// `timeout` at most; the bytes it moved.
        fn finish(
            &self,
            event: &Event,
            overlapped: &OVERLAPPED,
            timeout: Option<Duration>,
        ) -> io::Result<usize> {
            let millis =
                timeout.map_or(u32::MAX, |t| t.as_millis().min(u32::MAX as u128 - 1) as u32);
            // SAFETY: the event and the overlapped block outlive the call.
            unsafe {
                if WaitForSingleObject(event.0, millis) == WAIT_TIMEOUT {
                    let _ = CancelIoEx(self.raw(), Some(overlapped));
                    let mut moved = 0u32;
                    let _ = GetOverlappedResult(self.raw(), overlapped, &mut moved, true);
                    return Err(io::ErrorKind::TimedOut.into());
                }
                let mut moved = 0u32;
                GetOverlappedResult(self.raw(), overlapped, &mut moved, false).map_err(io_error)?;
                Ok(moved as usize)
            }
        }
    }

    impl Read for Pipe {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let timeout = *self
                .timeout
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let event = Event::new()?;
            let mut overlapped = OVERLAPPED {
                hEvent: event.0,
                ..Default::default()
            };
            let mut moved = 0u32;
            // SAFETY: buf, the overlapped block and the event live until the
            // call is finished or cancelled below.
            let started = unsafe {
                ReadFile(
                    self.raw(),
                    Some(buf),
                    Some(&mut moved),
                    Some(&mut overlapped),
                )
            };
            match started {
                Ok(()) => Ok(moved as usize),
                Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => {
                    self.finish(&event, &overlapped, timeout)
                }
                Err(e) => {
                    let e = io_error(e);
                    // The other end closed, or is closing: as a socket,
                    // nothing more to read.
                    const ERROR_BROKEN_PIPE: i32 = 109;
                    const ERROR_NO_DATA: i32 = 232;
                    const ERROR_PIPE_NOT_CONNECTED: i32 = 233;
                    if matches!(
                        e.raw_os_error(),
                        Some(ERROR_BROKEN_PIPE | ERROR_NO_DATA | ERROR_PIPE_NOT_CONNECTED)
                    ) {
                        Ok(0)
                    } else {
                        Err(e)
                    }
                }
            }
        }
    }

    impl Write for Pipe {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let event = Event::new()?;
            let mut overlapped = OVERLAPPED {
                hEvent: event.0,
                ..Default::default()
            };
            let mut moved = 0u32;
            // SAFETY: as in `read`.
            let started = unsafe {
                WriteFile(
                    self.raw(),
                    Some(buf),
                    Some(&mut moved),
                    Some(&mut overlapped),
                )
            };
            match started {
                Ok(()) => Ok(moved as usize),
                Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => {
                    self.finish(&event, &overlapped, None)
                }
                Err(e) => Err(io_error(e)),
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}
