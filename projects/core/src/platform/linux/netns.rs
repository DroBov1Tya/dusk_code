//! A program in a user and network namespace of its own, whose only reachable socket is
//! its broker's. Landlock can keep a program to a port number but not to an address, so
//! a program that may reach the broker would reach that port anywhere; in a namespace
//! with nothing but its own loopback, nothing else is there.
//!
//! Just before the program runs, its process leaves the machine's network, brings its
//! loopback up and listens on `127.0.0.1:PROXY_PORT`, then hands that socket (it keeps
//! the namespace it was made in) and a handle on its own process to the core over a
//! socket pair, and closes its copies. The core accepts on it as the broker, until the
//! process ends.
//!
//! Where unprivileged user namespaces are off, or the kernel lets a program make one but
//! not bring its loopback up (Ubuntu's AppArmor), `available` says so: it runs the whole
//! setup once.

use std::{
    ffi::CStr,
    io::{self, IoSliceMut},
    net::TcpListener,
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
    process::Stdio,
    sync::OnceLock,
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use nix::{
    libc,
    poll::{PollFd, PollFlags, PollTimeout, poll},
    sys::socket::{
        AddressFamily, ControlMessageOwned, MsgFlags, SockFlag, SockType, recvmsg, socketpair,
    },
};
use tracing::debug;

/// The broker's port inside the namespace. The namespace is new, so the port is free,
/// and the proxy variables can name it before the program starts.
pub const PROXY_PORT: u16 = 3128;
/// How long the core waits for the started program's socket.
const RECEIVE_TIME: Duration = Duration::from_secs(5);

/// Whether this Linux lets a program have a network of its own; asked once.
pub fn available() -> bool {
    static PROBE: OnceLock<bool> = OnceLock::new();
    *PROBE.get_or_init(|| match probe() {
        Ok(()) => true,
        Err(error) => {
            debug!("no network namespace for plugins: {error:#}");
            false
        }
    })
}

/// Runs `/bin/true` through the same setup a program gets.
fn probe() -> Result<()> {
    let mut command = std::process::Command::new("/bin/true");
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let receiver = prepare(&mut command)?;
    let mut child = command
        .spawn()
        .context("cannot start a program in a namespace")?;
    // Drops the child's end of the pair, so a failed start is seen at once.
    drop(command);
    let inside = receiver.listener();
    let status = child.wait()?;
    inside?;
    ensure!(status.success(), "the probe failed: {status}");
    Ok(())
}

/// Makes `command` run in a namespace of its own, listening for its broker; call
/// [`Receiver::listener`] once it has been spawned. Register it before any other
/// confinement of the process: Landlock would close `/proc` to it.
pub fn enter(command: &mut tokio::process::Command) -> Result<Receiver> {
    prepare(command.as_std_mut())
}

/// The core's end of the pair the started program sends its socket over.
pub struct Receiver {
    socket: OwnedFd,
}

/// What the started program handed over.
pub struct Inside {
    /// Listening on `127.0.0.1:PROXY_PORT` in the program's namespace.
    pub listener: TcpListener,
    /// A pidfd of the program's process: readable once it has ended.
    pub process: OwnedFd,
}

impl Receiver {
    /// Waits, at most five seconds, for the socket the program made. Blocks.
    pub fn listener(self) -> Result<Inside> {
        let mut fds = [PollFd::new(
            std::os::fd::AsFd::as_fd(&self.socket),
            PollFlags::POLLIN,
        )];
        let timeout = PollTimeout::try_from(RECEIVE_TIME).unwrap_or(PollTimeout::MAX);
        let ready = poll(&mut fds, timeout).context("cannot wait for the program")?;
        ensure!(ready > 0, "the program did not start in time");
        let mut byte = [0u8; 1];
        let mut iov = [IoSliceMut::new(&mut byte)];
        let mut space = nix::cmsg_space!([RawFd; 2]);
        let message = recvmsg::<()>(
            self.socket.as_raw_fd(),
            &mut iov,
            Some(&mut space),
            MsgFlags::MSG_CMSG_CLOEXEC,
        )
        .context("cannot receive the program's socket")?;
        let mut received = Vec::new();
        for control in message
            .cmsgs()
            .context("cannot read the program's socket")?
        {
            if let ControlMessageOwned::ScmRights(fds) = control {
                // SAFETY: the kernel just gave these descriptors to this process.
                received.extend(
                    fds.into_iter()
                        .map(|fd| unsafe { OwnedFd::from_raw_fd(fd) }),
                );
            }
        }
        let [listener, process] = <[OwnedFd; 2]>::try_from(received).map_err(|_| {
            anyhow::anyhow!("the program did not start: its network could not be made")
        })?;
        let listener = TcpListener::from(listener);
        let address = listener.local_addr()?;
        ensure!(
            address.ip().is_loopback() && address.port() == PROXY_PORT,
            "the program handed over another socket: {address}"
        );
        Ok(Inside { listener, process })
    }
}

/// What the child's step before exec needs, made in the parent: it may not allocate.
struct Prepared {
    uid_map: Vec<u8>,
    gid_map: Vec<u8>,
    loopback: libc::ifreq,
    address: libc::sockaddr_in,
    /// The child's end of the pair; closed in the parent when the command is dropped.
    sender: OwnedFd,
}

// SAFETY: `ifreq` holds a pointer only in a union field this code never sets; the
// rest are plain bytes and a descriptor.
unsafe impl Send for Prepared {}
unsafe impl Sync for Prepared {}

fn prepare(command: &mut std::process::Command) -> Result<Receiver> {
    use std::os::unix::process::CommandExt;
    let (socket, sender) = socketpair(
        AddressFamily::Unix,
        SockType::Stream,
        None,
        SockFlag::SOCK_CLOEXEC,
    )
    .context("cannot make the pair for the program's socket")?;
    // SAFETY: getuid and getgid cannot fail.
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    // SAFETY: all-zero is a valid ifreq and sockaddr_in.
    let mut loopback: libc::ifreq = unsafe { std::mem::zeroed() };
    for (slot, byte) in loopback.ifr_name.iter_mut().zip(b"lo") {
        *slot = *byte as libc::c_char;
    }
    loopback.ifr_ifru.ifru_flags = (libc::IFF_UP | libc::IFF_RUNNING) as libc::c_short;
    let mut address: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    address.sin_family = libc::AF_INET as libc::sa_family_t;
    address.sin_port = PROXY_PORT.to_be();
    address.sin_addr.s_addr = u32::from_be_bytes([127, 0, 0, 1]).to_be();
    let prepared = Prepared {
        uid_map: format!("{uid} {uid} 1\n").into_bytes(),
        gid_map: format!("{gid} {gid} 1\n").into_bytes(),
        loopback,
        address,
        sender,
    };
    // SAFETY: the closure runs in the forked child before exec and makes only system
    // calls, on memory made above; it allocates nothing.
    unsafe {
        command.pre_exec(move || inside(&prepared));
    }
    Ok(Receiver { socket })
}

fn check(result: libc::c_int) -> io::Result<libc::c_int> {
    match result {
        -1 => Err(io::Error::last_os_error()),
        done => Ok(done),
    }
}

/// In the child, before exec: the namespace, its loopback, the socket handed over.
fn inside(prepared: &Prepared) -> io::Result<()> {
    // SAFETY: plain system calls on valid buffers; the descriptors are the child's own.
    unsafe {
        check(libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNET))?;
        // Without it an unprivileged process may not write its group map.
        write_file(c"/proc/self/setgroups", b"deny")?;
        write_file(c"/proc/self/uid_map", &prepared.uid_map)?;
        write_file(c"/proc/self/gid_map", &prepared.gid_map)?;
        let control = check(libc::socket(
            libc::AF_INET,
            libc::SOCK_DGRAM | libc::SOCK_CLOEXEC,
            0,
        ))?;
        let up = libc::ioctl(control, libc::SIOCSIFFLAGS as _, &prepared.loopback);
        libc::close(control);
        check(up)?;
        let listener = check(libc::socket(
            libc::AF_INET,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC,
            0,
        ))?;
        check(libc::bind(
            listener,
            (&raw const prepared.address).cast(),
            size_of::<libc::sockaddr_in>() as libc::socklen_t,
        ))?;
        check(libc::listen(listener, 64))?;
        // Closed on exec, like every descriptor made here.
        let process = libc::syscall(libc::SYS_pidfd_open, libc::getpid(), 0);
        check(process as libc::c_int)?;
        let process = process as libc::c_int;
        send(prepared.sender.as_raw_fd(), [listener, process])?;
        // The program keeps none of them: the broker's socket is the core's alone.
        libc::close(listener);
        libc::close(process);
        libc::close(prepared.sender.as_raw_fd());
    }
    Ok(())
}

/// Writes `bytes` to the file at `path` in one call.
unsafe fn write_file(path: &CStr, bytes: &[u8]) -> io::Result<()> {
    // SAFETY: `path` is a C string and `bytes` a valid buffer.
    unsafe {
        let file = check(libc::open(path.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC))?;
        let written = libc::write(file, bytes.as_ptr().cast(), bytes.len());
        let error = io::Error::last_os_error();
        libc::close(file);
        match written {
            -1 => Err(error),
            done if done as usize == bytes.len() => Ok(()),
            _ => Err(io::Error::from_raw_os_error(libc::EIO)),
        }
    }
}

/// Sends the two descriptors over `socket` with one byte.
unsafe fn send(socket: RawFd, fds: [RawFd; 2]) -> io::Result<()> {
    // Room for one header and two descriptors, aligned for the header.
    let mut space = [0u64; 8];
    let mut byte = [0u8; 1];
    let mut iov = libc::iovec {
        iov_base: byte.as_mut_ptr().cast(),
        iov_len: 1,
    };
    // SAFETY: the header points at buffers that outlive the call; CMSG_* stay within
    // `space`, which is larger than CMSG_SPACE of two descriptors.
    unsafe {
        let length = libc::CMSG_LEN(size_of::<[RawFd; 2]>() as u32);
        let mut message: libc::msghdr = std::mem::zeroed();
        message.msg_iov = &mut iov;
        message.msg_iovlen = 1;
        message.msg_control = space.as_mut_ptr().cast();
        message.msg_controllen = libc::CMSG_SPACE(size_of::<[RawFd; 2]>() as u32) as _;
        let header = libc::CMSG_FIRSTHDR(&message);
        if header.is_null() {
            return Err(io::Error::from_raw_os_error(libc::EINVAL));
        }
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = length as _;
        std::ptr::copy_nonoverlapping(fds.as_ptr(), libc::CMSG_DATA(header).cast(), 2);
        let sent = libc::sendmsg(socket, &message, libc::MSG_NOSIGNAL);
        if sent == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::broker::{Broker, Origin, Policy, new_token};

    /// The probe and a real entry agree: where unshare works but loopback cannot come
    /// up, both fail.
    #[tokio::test]
    async fn the_probe_runs_the_whole_namespace_setup() {
        let mut command = tokio::process::Command::new("/bin/true");
        let entered = async {
            let receiver = enter(&mut command)?;
            let mut child = command.spawn()?;
            let inside = receiver.listener();
            ensure!(child.wait().await?.success());
            inside.map(|_| ())
        }
        .await;
        assert_eq!(available(), entered.is_ok(), "{entered:?}");
    }

    /// Spec 8, test 3 on Linux: in the namespace, any address but the broker's socket is
    /// unreachable, and the broker keeps its policy there.
    #[tokio::test]
    async fn inside_the_namespace_only_the_broker_answers() {
        if !available() {
            eprintln!("no user namespaces here; skipped");
            return;
        }
        // A server on the machine's own loopback: listed, yet not in the namespace.
        let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = server.local_addr().unwrap().port();
        tokio::spawn(async move { while server.accept().await.is_ok() {} });
        let listed = Origin::parse(&format!("https://127.0.0.1:{port}")).unwrap();
        let token = new_token().unwrap();
        let mut policy = Policy::new(token.clone(), vec![listed], Vec::new());
        policy.public_only = false;
        let broker = Broker::start(policy).unwrap();
        let gate = broker.gate();
        let auth = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            format!("dusk:{}", token.0),
        );
        let run = async |script: String| {
            let mut command = tokio::process::Command::new("bash");
            command.arg("-c").arg(script).stdout(Stdio::piped());
            let receiver = enter(&mut command).unwrap();
            let child = command.spawn().unwrap();
            drop(command);
            let inside = receiver.listener().unwrap();
            let serving = gate.serve(inside.listener).unwrap();
            let output = child.wait_with_output().await.unwrap();
            serving.abort();
            (
                output.status.success(),
                String::from_utf8_lossy(&output.stdout).into_owned(),
            )
        };
        let reach = |address: &str| format!("exec 3<>/dev/tcp/{address}");
        // No route out, and the machine's loopback is not this one.
        assert!(!run(reach("1.1.1.1/443")).await.0);
        assert!(!run(reach(&format!("127.0.0.1/{port}"))).await.0);
        let connect = |target: &str| {
            format!(
                "{} && printf 'CONNECT {target} HTTP/1.1\\r\\nProxy-Authorization: Basic \
                 {auth}\\r\\n\\r\\n' >&3 && read -r line <&3 && echo \"$line\"",
                reach(&format!("127.0.0.1/{PROXY_PORT}"))
            )
        };
        let (ok, answer) = run(connect(&format!("127.0.0.1:{port}"))).await;
        assert!(ok && answer.starts_with("HTTP/1.1 200"), "{answer}");
        let (_, answer) = run(connect("other.example:443")).await;
        assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    }
}
