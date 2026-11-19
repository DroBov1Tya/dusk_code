//! The kinds of socket a program in its own network namespace may open: IPv4, IPv6 and
//! netlink. The namespace closes every address but the broker's, yet not the Unix
//! sockets on disk (the D-Bus session bus, through which `systemd-run --user` starts
//! anything unconfined, the Secret Service, docker, tmux, agents, X11) nor vsock to the
//! machine's virtual machines; Landlock does not reach them either. A seccomp filter
//! refuses those sockets before they exist.
//!
//! The filter gates making a socket, not using one already open, so every descriptor
//! but stdin, stdout and stderr is first marked to close on exec: one the core holds
//! without that mark (a leak of a library, one handed down by its own parent) does not
//! reach the program. The netns step has already handed its sockets over and closed them.
//!
//! The filter is made in the parent; the child only installs it, last before exec.

use std::{io, os::unix::process::CommandExt};

use anyhow::Result;
use nix::libc;

/// `AUDIT_ARCH_*` of the architectures this filter knows; any other call is killed.
#[cfg(target_arch = "x86_64")]
const ARCH: u32 = 0xC000_003E;
#[cfg(target_arch = "aarch64")]
const ARCH: u32 = 0xC000_00B7;
/// x32 calls on x86_64 share the architecture and carry this bit in their number.
const X32_BIT: u32 = 0x4000_0000;
/// Offsets in `seccomp_data`: the call's number, its architecture, its first argument
/// (the low half on these little-endian machines).
const NR: u32 = 0;
const ARCH_OFFSET: u32 = 4;
const FIRST_ARG: u32 = 16;
const ALLOWED_FAMILIES: [libc::c_int; 3] = [libc::AF_INET, libc::AF_INET6, libc::AF_NETLINK];

/// Adds the filter as the last step before `command` runs.
pub fn confine(command: &mut std::process::Command) -> Result<()> {
    let filter = Filter(program()?);
    // SAFETY: the closure runs in the forked child before exec and makes only the two
    // prctl calls, on a program made here; it allocates nothing.
    unsafe {
        command.pre_exec(move || filter.install());
    }
    Ok(())
}

struct Filter(Vec<libc::sock_filter>);

impl Filter {
    fn install(&self) -> io::Result<()> {
        let program = libc::sock_fprog {
            len: self.0.len() as libc::c_ushort,
            filter: self.0.as_ptr().cast_mut(),
        };
        // SAFETY: plain system calls; `program` points at instructions that outlive the
        // call, and the kernel copies them.
        unsafe {
            close_on_exec_from(3)?;
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) == -1
                || libc::prctl(
                    libc::PR_SET_SECCOMP,
                    libc::SECCOMP_MODE_FILTER,
                    &raw const program,
                ) == -1
            {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }
}

/// Marks every descriptor from `first` up to close on exec, allocating nothing.
unsafe fn close_on_exec_from(first: libc::c_int) -> io::Result<()> {
    // SAFETY: system calls on descriptors only.
    unsafe {
        let all = libc::syscall(
            libc::SYS_close_range,
            first as libc::c_uint,
            libc::c_uint::MAX,
            libc::CLOSE_RANGE_CLOEXEC,
        );
        if all == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        // Kernels before 5.11 have no close_range, 5.10 not its flag: one at a time.
        if !matches!(error.raw_os_error(), Some(libc::ENOSYS | libc::EINVAL)) {
            return Err(error);
        }
        let mut limit: libc::rlimit = std::mem::zeroed();
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) == -1 {
            return Err(io::Error::last_os_error());
        }
        let last = limit.rlim_cur.min(libc::c_int::MAX as libc::rlim_t) as libc::c_int;
        for fd in first..last {
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags != -1 {
                libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC);
            }
        }
    }
    Ok(())
}

fn statement(code: u32, k: u32) -> libc::sock_filter {
    libc::sock_filter {
        code: code as u16,
        jt: 0,
        jf: 0,
        k,
    }
}

fn jump(code: u32, k: u32, jt: u8, jf: u8) -> libc::sock_filter {
    libc::sock_filter {
        code: code as u16,
        jt,
        jf,
        k,
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn program() -> Result<Vec<libc::sock_filter>> {
    use libc::{BPF_ABS, BPF_JEQ, BPF_JMP, BPF_JSET, BPF_K, BPF_LD, BPF_RET, BPF_W};
    let load = |offset| statement(BPF_LD | BPF_W | BPF_ABS, offset);
    let ret = |action| statement(BPF_RET | BPF_K, action);
    let errno = |code: libc::c_int| ret(libc::SECCOMP_RET_ERRNO | code as u32);
    let mut filter = vec![
        load(ARCH_OFFSET),
        jump(BPF_JMP | BPF_JEQ | BPF_K, ARCH, 1, 0),
        ret(libc::SECCOMP_RET_KILL_PROCESS),
        load(NR),
    ];
    if cfg!(target_arch = "x86_64") {
        filter.push(jump(BPF_JMP | BPF_JSET | BPF_K, X32_BIT, 0, 1));
        filter.push(errno(libc::EPERM));
    }
    // io_uring opens sockets without the socket call; socketpair makes Unix ones.
    for refused in [libc::SYS_io_uring_setup, libc::SYS_socketpair] {
        filter.push(jump(BPF_JMP | BPF_JEQ | BPF_K, refused as u32, 0, 1));
        filter.push(errno(libc::EPERM));
    }
    filter.push(jump(
        BPF_JMP | BPF_JEQ | BPF_K,
        libc::SYS_socket as u32,
        1,
        0,
    ));
    filter.push(ret(libc::SECCOMP_RET_ALLOW));
    filter.push(load(FIRST_ARG));
    let families = ALLOWED_FAMILIES.len() as u8;
    for (index, family) in ALLOWED_FAMILIES.iter().enumerate() {
        // Each allowed family jumps past the remaining checks and the refusal.
        let past = families - index as u8;
        filter.push(jump(BPF_JMP | BPF_JEQ | BPF_K, *family as u32, past, 0));
    }
    filter.push(errno(libc::EAFNOSUPPORT));
    filter.push(ret(libc::SECCOMP_RET_ALLOW));
    Ok(filter)
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn program() -> Result<Vec<libc::sock_filter>> {
    anyhow::bail!(
        "this machine's architecture has no socket filter, so no program is kept to its broker"
    )
}
