//! What this Windows must give before Kule runs a command in a sandbox,
//! checked once and named for the person when it does not: a build with
//! the process-container interfaces Kule's sandbox is made of, a desktop
//! session (a service's or a remote shell's session cannot start a
//! container that runs the usual programs), and a container that holds
//! the paths it is told to deny. A gap here is no weaker mode to accept:
//! nothing runs until it is closed (see [`crate::Sandbox::start`]).
//!
//! Measured on a real machine before Kule depended on it
//! (`docs/windows-measurements.md`): the build where the measurements were
//! taken is the oldest Kule accepts.

use std::sync::OnceLock;

/// The oldest Windows build whose process container Kule measured:
/// Windows 11 24H2 (26100) with the update that brought its revision to
/// this.
pub const LEAST_BUILD: (u32, u32) = (26100, 9278);

/// Why a sandboxed command cannot run on this computer, one sentence
/// each; empty where everything is in place. Checked once a process.
pub fn check() -> Result<(), Vec<String>> {
    static CHECKED: OnceLock<Vec<String>> = OnceLock::new();
    let reasons = CHECKED.get_or_init(check_now);
    if reasons.is_empty() {
        Ok(())
    } else {
        Err(reasons.clone())
    }
}

fn check_now() -> Vec<String> {
    let mut reasons = Vec::new();
    let (build, revision) = build();
    if (build, revision) < LEAST_BUILD {
        reasons.push(format!(
            "Windows 11 24H2 with update {}.{} or later is needed (this computer runs {build}.{revision})",
            LEAST_BUILD.0, LEAST_BUILD.1
        ));
    }
    if !interactive_session() {
        reasons.push(
            "sandboxed commands start only from your desktop session: this one is a service's or a remote shell's, where the programs a task runs cannot start"
                .to_owned(),
        );
    }
    let support = mxc_sdk::v1::platform_support();
    if !support.is_supported {
        reasons.push(format!(
            "this Windows gives no process container: {}",
            support
                .reason
                .unwrap_or_else(|| "no reason given".to_owned())
        ));
        return reasons;
    }
    let request = mxc_sdk::v1::ContainerRequest {
        containment: mxc_sdk::v1::Containment::ProcessContainer(Default::default()),
        ..mxc_sdk::v1::ContainerRequest::new("cmd.exe /d /c exit 0")
    };
    match mxc_sdk::v1::probe(Some(&request)) {
        Ok(out) => {
            if let Some(error) = out.error {
                reasons.push(format!("this Windows gives no process container: {error}"));
            } else if out.tier.is_none() {
                reasons.push("this Windows gives no process container of any kind".to_owned());
            }
            if !out.probes.base_container_supports_deny_paths {
                reasons.push(
                    "this Windows's process container cannot keep a path from a command: Kule hides your credentials that way"
                        .to_owned(),
                );
            }
            if !out.probes.base_container_supports_enumerate_paths {
                reasons.push(
                    "this Windows's process container cannot let a command see the way to its folder without its contents"
                        .to_owned(),
                );
            }
        }
        Err(e) => reasons.push(format!(
            "this Windows gives no process container: {}",
            e.message
        )),
    }
    reasons
}

/// This Windows's build and update revision (`26100.9278`), from the kernel
/// and the registry; zero where either cannot be read.
pub fn build() -> (u32, u32) {
    use windows::Wdk::System::SystemServices::RtlGetVersion;
    use windows::Win32::System::SystemInformation::OSVERSIONINFOW;

    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // SAFETY: the structure says its own size; RtlGetVersion fills it.
    let status = unsafe { RtlGetVersion(&mut info) };
    let build = if status.is_ok() {
        info.dwBuildNumber
    } else {
        0
    };
    (build, revision().unwrap_or(0))
}

/// The update revision (`UBR`) from the registry.
fn revision() -> Option<u32> {
    use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RegGetValueW};
    use windows::core::w;

    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: the value and its size are given; the key and name are
    // constant strings.
    let result = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"),
            w!("UBR"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    result.is_ok().then_some(value)
}

/// Whether this process runs in a desktop session: not session 0, where
/// services and remote shells run, and on the interactive window station.
pub fn interactive_session() -> bool {
    use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows::Win32::System::StationsAndDesktops::{
        GetProcessWindowStation, GetUserObjectInformationW, UOI_NAME,
    };
    use windows::Win32::System::Threading::GetCurrentProcessId;

    let mut session = 0u32;
    // SAFETY: only reads the session of this process.
    if unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) }.is_err() || session == 0
    {
        return false;
    }
    // SAFETY: the station handle is this process's own and not closed; the
    // name is read into a buffer of the given size.
    unsafe {
        let Ok(station) = GetProcessWindowStation() else {
            return false;
        };
        let mut name = [0u16; 64];
        let mut needed = 0u32;
        if GetUserObjectInformationW(
            windows::Win32::Foundation::HANDLE(station.0),
            UOI_NAME,
            Some(name.as_mut_ptr() as *mut _),
            (name.len() * 2) as u32,
            Some(&mut needed),
        )
        .is_err()
        {
            return false;
        }
        let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        String::from_utf16_lossy(&name[..end]).eq_ignore_ascii_case("WinSta0")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The build is read on every Windows this runs on; the revision too,
    /// on every Windows 10 or later.
    #[test]
    fn the_build_and_revision_are_read() {
        let (build, revision) = build();
        assert!(build >= 10240, "{build}");
        assert!(revision > 0, "{revision}");
    }

    /// Every reason is one sentence a person can act on, and the check
    /// gives the same answer each time.
    #[test]
    fn the_check_names_each_gap_once_and_the_same_way_each_time() {
        let first = check();
        let second = check();
        assert_eq!(first, second);
        if let Err(reasons) = first {
            for reason in reasons {
                assert!(!reason.is_empty() && !reason.ends_with('.'), "{reason}");
            }
        }
    }
}
