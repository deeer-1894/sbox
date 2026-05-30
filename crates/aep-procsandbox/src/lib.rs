//! Level-2 process sandbox: run a closure in an isolated child whose network
//! syscalls are denied by a seccomp filter. Linux-only; a stub elsewhere.

use std::io::{Error, ErrorKind, Result};

/// Run `f` in a forked child with a network-lockdown seccomp filter applied.
/// Returns the child's exit code. Linux-only.
#[cfg(target_os = "linux")]
pub fn run_isolated<F: FnOnce() -> i32>(f: F) -> Result<i32> {
    // SAFETY: the child runs an alloc-free closure and _exit; the parent only
    // waitpids. fork is the documented mechanism for an isolated sandbox process.
    match unsafe { libc::fork() } {
        -1 => Err(Error::last_os_error()),
        0 => {
            let code = match apply_network_lockdown() {
                Ok(()) => f(),
                Err(_) => 111,
            };
            unsafe { libc::_exit(code) }
        }
        pid => {
            let mut status: libc::c_int = 0;
            if unsafe { libc::waitpid(pid, &mut status, 0) } < 0 {
                return Err(Error::last_os_error());
            }
            if libc::WIFEXITED(status) {
                Ok(libc::WEXITSTATUS(status))
            } else {
                Err(Error::new(ErrorKind::Other, "sandboxed child did not exit normally"))
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn apply_network_lockdown() -> Result<()> {
    use seccompiler::{BpfProgram, SeccompAction, SeccompFilter};
    use std::collections::BTreeMap;

    #[cfg(target_arch = "aarch64")]
    const ARCH: seccompiler::TargetArch = seccompiler::TargetArch::aarch64;
    #[cfg(target_arch = "x86_64")]
    const ARCH: seccompiler::TargetArch = seccompiler::TargetArch::x86_64;

    // Required to install a seccomp filter unprivileged.
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
        return Err(Error::last_os_error());
    }

    // Deny socket creation (network) with EPERM; allow everything else.
    let rules = BTreeMap::from([(libc::SYS_socket, vec![])]);
    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        ARCH,
    )
    .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
    let prog: BpfProgram = filter
        .try_into()
        .map_err(|e: seccompiler::BackendError| Error::new(ErrorKind::Other, e.to_string()))?;
    seccompiler::apply_filter(&prog).map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
    Ok(())
}

/// Non-Linux stub so the workspace builds on the dev host (macOS).
#[cfg(not(target_os = "linux"))]
pub fn run_isolated<F: FnOnce() -> i32>(_f: F) -> Result<i32> {
    Err(Error::new(ErrorKind::Unsupported, "process sandbox requires Linux"))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn sandboxed_compute_runs() {
        assert_eq!(run_isolated(|| 2 + 3).unwrap(), 5);
    }

    #[test]
    fn socket_is_denied_in_sandbox() {
        let code = run_isolated(|| {
            let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
            if fd < 0 {
                0 // denied (EPERM) — expected
            } else {
                unsafe { libc::close(fd) };
                1 // not denied — sandbox failed
            }
        })
        .unwrap();
        assert_eq!(code, 0, "socket() must be denied in the sandbox");
    }

    #[test]
    fn socket_works_without_sandbox() {
        // Contrast: the same syscall succeeds when not sandboxed.
        let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
        assert!(fd >= 0, "socket() works normally outside the sandbox");
        unsafe { libc::close(fd) };
    }
}
