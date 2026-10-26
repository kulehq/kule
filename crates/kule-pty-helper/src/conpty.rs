//! The pseudo console and the shell on it. Kule's frames come in on our
//! input; what the console shows goes out on our output as it comes. The
//! shell's end is ours: its exit code, once the console has shown all it
//! had. Our input ending (Kule closed the terminal) ends the shell.

use std::ffi::{OsStr, c_void};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kule_pty_helper::{Args, Frame, clamp, read_frame};
use windows::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::System::Console::{
    COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole,
};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, INFINITE, InitializeProcThreadAttributeList,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_INFORMATION,
    STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject,
};
use windows::core::{PCWSTR, PWSTR};

/// How long what the console showed last is waited for once the shell
/// ended.
const DRAIN: Duration = Duration::from_secs(2);

fn handle(h: &impl AsRawHandle) -> HANDLE {
    HANDLE(h.as_raw_handle())
}

/// A pipe's read and write ends, neither inherited by anything.
fn pipe() -> io::Result<(File, File)> {
    let (mut read, mut write) = (HANDLE::default(), HANDLE::default());
    // SAFETY: CreatePipe fills both handles, which nothing else owns.
    unsafe {
        CreatePipe(&mut read, &mut write, None, 0)?;
        Ok((
            File::from_raw_handle(read.0),
            File::from_raw_handle(write.0),
        ))
    }
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}

fn coord(cols: u16, rows: u16) -> COORD {
    let (cols, rows) = clamp(cols, rows);
    COORD {
        X: cols as i16,
        Y: rows as i16,
    }
}

/// The console, closed once: by whoever ends it first, never resized after.
struct Console(Mutex<Option<HPCON>>);

impl Console {
    fn resize(&self, cols: u16, rows: u16) {
        let held = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(hpc) = *held {
            // SAFETY: open while held under the lock.
            let _ = unsafe { ResizePseudoConsole(hpc, coord(cols, rows)) };
        }
    }

    fn close(&self) {
        let mut held = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(hpc) = held.take() {
            // SAFETY: made by CreatePseudoConsole, closed this once.
            unsafe { ClosePseudoConsole(hpc) };
        }
    }
}

/// Runs the shell `args` names on a pseudo console of its size, to its end.
pub fn run(args: &Args) -> io::Result<u32> {
    let (console_in, to_console) = pipe()?;
    let (from_console, console_out) = pipe()?;
    // SAFETY: both handles are open; the console takes its own of each.
    let hpc = unsafe {
        CreatePseudoConsole(
            coord(args.cols, args.rows),
            handle(&console_in),
            handle(&console_out),
            0,
        )?
    };
    // Ours are not needed: once the console is closed its output ends.
    drop((console_in, console_out));
    let console = Arc::new(Console(Mutex::new(Some(hpc))));
    let shell = match start(hpc, args) {
        Ok(shell) => Arc::new(shell),
        Err(e) => {
            console.close();
            return Err(e);
        }
    };

    let (shown, all_shown) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        let mut from_console = from_console;
        let mut out = io::stdout().lock();
        let mut buf = [0u8; 16 * 1024];
        while let Ok(n) = from_console.read(&mut buf) {
            if n == 0 || out.write_all(&buf[..n]).and_then(|()| out.flush()).is_err() {
                break;
            }
        }
        let _ = shown.send(());
    });

    let (typed, ended) = (Arc::clone(&console), Arc::clone(&shell));
    std::thread::spawn(move || {
        let mut to_console = to_console;
        let mut input = io::stdin().lock();
        loop {
            match read_frame(&mut input) {
                Ok(Some(Frame::Data(bytes))) => {
                    if to_console.write_all(&bytes).is_err() {
                        break;
                    }
                }
                Ok(Some(Frame::Resize { cols, rows })) => typed.resize(cols, rows),
                // Kule closed the terminal, or sent what it never sends:
                // either way the shell ends.
                Ok(None) | Err(_) => break,
            }
        }
        // SAFETY: the process handle stays open while `ended` holds it.
        let _ = unsafe { TerminateProcess(handle(&*ended), 1) };
    });

    // SAFETY: the process handle is open.
    unsafe { WaitForSingleObject(handle(&*shell), INFINITE) };
    let mut code = 0u32;
    // SAFETY: the process ended; its handle is open.
    unsafe { GetExitCodeProcess(handle(&*shell), &mut code)? };
    // The console's output ends only once it is closed.
    console.close();
    let _ = all_shown.recv_timeout(DRAIN);
    Ok(code)
}

/// Starts the shell on console `hpc`, attached to it alone: no handle of
/// ours is inherited, its standard streams included.
fn start(hpc: HPCON, args: &Args) -> io::Result<OwnedHandle> {
    let mut size = 0usize;
    // SAFETY: asks only for the size the list needs.
    let _ = unsafe { InitializeProcThreadAttributeList(None, 1, None, &mut size) };
    // u64s, so the list is aligned as it must be.
    let mut room = vec![0u64; size.div_ceil(8)];
    let list = LPPROC_THREAD_ATTRIBUTE_LIST(room.as_mut_ptr().cast::<c_void>());
    // SAFETY: `room` is `size` bytes at least, and outlives the list.
    unsafe { InitializeProcThreadAttributeList(Some(list), 1, None, &mut size)? };
    let made = (|| -> io::Result<OwnedHandle> {
        // SAFETY: the console's handle is the attribute's value itself.
        unsafe {
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                Some(hpc.0 as *const c_void),
                std::mem::size_of::<HPCON>(),
                None,
                None,
            )?
        };
        let mut info = STARTUPINFOEXW::default();
        info.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        // Without this the shell would take our own (piped) streams rather
        // than the console's.
        info.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        info.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
        info.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
        info.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
        info.lpAttributeList = list;
        let program = wide(args.program.as_os_str());
        let mut line = wide(&args.line);
        let mut process = PROCESS_INFORMATION::default();
        // SAFETY: the strings end in NUL and live past the call; the line
        // is writable, as CreateProcessW wants it.
        unsafe {
            CreateProcessW(
                PCWSTR(program.as_ptr()),
                Some(PWSTR(line.as_mut_ptr())),
                None,
                None,
                false,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                None,
                PCWSTR::null(),
                &info.StartupInfo,
                &mut process,
            )
            .map_err(|e| {
                io::Error::other(format!("{} could not start: {e}", args.program.display()))
            })?;
            drop(OwnedHandle::from_raw_handle(process.hThread.0));
            Ok(OwnedHandle::from_raw_handle(process.hProcess.0))
        }
    })();
    // SAFETY: initialised above, deleted this once.
    unsafe { DeleteProcThreadAttributeList(list) };
    made
}
