//! A Seatbelt profile written for each command: everything allowed but writing outside
//! the open folders, any access to the hidden ones and, as the confinement says, the
//! network, the keychain and programs outside the system's own. `sandbox-exec` is
//! deprecated in name only; the system and Apple's own tools still use it.

use std::path::Path;

use anyhow::{Result, ensure};
use tokio::process::Command;

use crate::platform::Confinement;

const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
/// Programs of the system, which SIP keeps from being changed; whatever they do stays
/// in the same confinement.
const SYSTEM_PROGRAMS: [&str; 7] = [
    "/bin",
    "/sbin",
    "/usr/bin",
    "/usr/sbin",
    "/usr/libexec",
    "/System",
    "/Library/Developer/CommandLineTools",
];
/// Where the keychain's services and files are.
const KEYCHAIN_SERVICES: &str = "(deny mach-lookup (global-name \"com.apple.SecurityServer\") \
     (global-name \"com.apple.securityd\") (global-name \"com.apple.securityd.xpc\") \
     (global-name-prefix \"com.apple.security.\"))\n";

/// The ways to have the system run something outside the sandbox: a launchd job, an
/// app or URL opened by LaunchServices, an Apple event to another app.
const HAND_OFF: &str = "(deny process-exec (literal \"/bin/launchctl\") \
     (literal \"/usr/bin/open\") (literal \"/usr/bin/osascript\") \
     (literal \"/usr/bin/osacompile\") (literal \"/usr/bin/automator\") \
     (literal \"/usr/bin/shortcuts\"))\n(deny lsopen)\n(deny appleevent-send)\n";

pub fn available() -> bool {
    Path::new(SANDBOX_EXEC).is_file()
}

pub fn command(program: &Path, arguments: &[String], confinement: &Confinement) -> Result<Command> {
    let mut sandboxed = Command::new(SANDBOX_EXEC);
    sandboxed
        .arg("-p")
        .arg(profile(confinement)?)
        .arg(program)
        .args(arguments);
    Ok(sandboxed)
}

fn profile(confinement: &Confinement) -> Result<String> {
    let mut profile = String::from(
        "(version 1)\n(allow default)\n(deny file-write*)\n(allow file-write*\n  \
         (literal \"/dev/null\") (literal \"/dev/zero\") (literal \"/dev/dtracehelper\")\n  \
         (regex #\"^/dev/tty\") (regex #\"^/dev/fd/\")",
    );
    for folder in &confinement.writable {
        profile.push_str(&format!("\n  (subpath {})", quoted(folder)?));
    }
    profile.push_str(")\n");
    for folder in &confinement.hidden {
        profile.push_str(&format!(
            "(deny file-read* file-write* (subpath {}))\n",
            quoted(folder)?
        ));
    }
    // After the hidden ones: the later rule wins in Seatbelt.
    for folder in &confinement.reopened {
        let access = match confinement.writable.contains(folder) {
            true => "file-read* file-write*",
            false => "file-read*",
        };
        profile.push_str(&format!("(allow {access} (subpath {}))\n", quoted(folder)?));
    }
    if !confinement.keychain {
        profile.push_str(KEYCHAIN_SERVICES);
        if let Some(home) = std::env::var_os("HOME") {
            let keychains = Path::new(&home).join("Library/Keychains");
            profile.push_str(&format!(
                "(deny file-read* file-write* (subpath {}))\n",
                quoted(&keychains)?
            ));
        }
    }
    if let Some(port) = confinement.network.only_port() {
        profile.push_str(&format!(
            "(deny network*)\n(allow network-outbound (remote ip \"localhost:{port}\"))\n"
        ));
    } else if !confinement.network.is_open() {
        profile.push_str("(deny network*)\n");
    }
    if !confinement.network.is_open() {
        for folder in &confinement.sockets {
            profile.push_str(&format!(
                "(allow network-outbound (remote unix-socket (subpath {})))\n",
                quoted(folder)?
            ));
        }
    }
    if let Some(programs) = &confinement.programs {
        profile.push_str("(deny process-exec)\n(allow process-exec");
        for folder in SYSTEM_PROGRAMS {
            profile.push_str(&format!("\n  (subpath \"{folder}\")"));
        }
        for program in programs {
            profile.push_str(&format!("\n  (subpath {})", quoted(program)?));
        }
        profile.push_str(")\n");
    }
    if !confinement.hand_off {
        // After the allow above: the later rule wins.
        profile.push_str(HAND_OFF);
    }
    Ok(profile)
}

/// A path as a profile string; one that could break out of the string is refused.
fn quoted(path: &Path) -> Result<String> {
    let Some(text) = path.to_str() else {
        anyhow::bail!("{} cannot be named in a sandbox profile", path.display());
    };
    ensure!(
        !text
            .chars()
            .any(|c| c.is_control() || c == '"' || c == '\\'),
        "{} cannot be named in a sandbox profile",
        path.display()
    );
    Ok(format!("\"{text}\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Network;

    fn run(confinement: &Confinement, program: &str, arguments: &[&str]) -> bool {
        let arguments: Vec<String> = arguments.iter().map(|word| (*word).to_owned()).collect();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let mut command = command(Path::new(program), &arguments, confinement).unwrap();
            command
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            command.status().await.unwrap().success()
        })
    }

    #[test]
    fn a_confined_command_writes_only_in_the_open_folder() {
        let root = crate::testing::temp_dir("seatbelt");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let (open, closed) = (root.join("open"), root.join("closed"));
        for folder in [&open, &closed] {
            std::fs::create_dir_all(folder).unwrap();
        }
        let confinement = Confinement {
            writable: vec![open.clone()],
            hidden: vec![closed.clone()],
            ..Confinement::around(&open, &open, None)
        };
        let shell = |script: String| run(&confinement, "/bin/sh", &["-c", &script]);
        assert!(shell(format!("echo yes > {}/file", open.display())));
        assert!(!shell(format!("echo no > {}/file", root.display())));
        assert!(!shell(format!("ls {}", closed.display())));
        assert!(quoted(Path::new("/a\"b")).is_err());
    }

    /// A plugin's program: the config root closed but its own folder readable and its
    /// data writable, no network, nothing started from a folder of the user's.
    #[test]
    fn a_plugin_reads_its_folder_but_not_the_config_root() {
        let root = crate::testing::temp_dir("seatbelt-plugin");
        let (plugin, data) = (root.join("rules/plugins/p"), root.join("plugin-data/p"));
        for folder in [&plugin, &data] {
            std::fs::create_dir_all(folder).unwrap();
        }
        let root = root.canonicalize().unwrap();
        let (plugin, data) = (plugin.canonicalize().unwrap(), data.canonicalize().unwrap());
        std::fs::write(root.join("config.yml"), "key: secret").unwrap();
        std::fs::write(plugin.join("own"), "mine").unwrap();
        let copied_cat = data.join("cat");
        std::fs::copy("/bin/cat", &copied_cat).unwrap();
        let confinement = Confinement {
            writable: vec![data.clone()],
            hidden: vec![root.clone()],
            reopened: vec![plugin.clone(), data.clone()],
            network: Network::Closed,
            keychain: false,
            programs: Some(vec![plugin.clone()]),
            sockets: Vec::new(),
            hand_off: true,
        };
        let read = |path: &Path| run(&confinement, "/bin/cat", &[&path.display().to_string()]);
        assert!(!read(&root.join("config.yml")), "the config root is closed");
        assert!(read(&plugin.join("own")), "its own folder is open");
        let write = format!("echo x > {}/state", data.display());
        assert!(run(&confinement, "/bin/sh", &["-c", &write]));
        let write_own = format!("echo x > {}/state", plugin.display());
        assert!(!run(&confinement, "/bin/sh", &["-c", &write_own]));
        let program = copied_cat.display().to_string();
        assert!(
            !run(&confinement, &program, &["/dev/null"]),
            "no program of its data"
        );
        assert!(!run(
            &confinement,
            "/usr/bin/nc",
            &["-z", "-w", "2", "1.1.1.1", "53"]
        ));
        // Only the broker's port on this machine.
        let broker = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let other = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = |listener: &std::net::TcpListener| listener.local_addr().unwrap().port();
        let confinement = Confinement {
            network: Network::Broker(port(&broker)),
            ..confinement
        };
        let reach = |port: u16| {
            let port = port.to_string();
            run(
                &confinement,
                "/usr/bin/nc",
                &["-z", "-w", "2", "127.0.0.1", &port],
            )
        };
        assert!(reach(port(&broker)));
        assert!(!reach(port(&other)));
        assert!(!run(
            &confinement,
            "/usr/bin/nc",
            &["-z", "-w", "2", "1.1.1.1", "53"]
        ));
        // A Unix socket only in a folder it names.
        let (named, other) = (root.join("vm"), root.join("other"));
        for folder in [&named, &other] {
            std::fs::create_dir_all(folder).unwrap();
        }
        let _named = std::os::unix::net::UnixListener::bind(named.join("s.sock")).unwrap();
        let _other = std::os::unix::net::UnixListener::bind(other.join("s.sock")).unwrap();
        let confinement = Confinement {
            network: Network::Closed,
            hidden: Vec::new(),
            sockets: vec![named.clone()],
            ..confinement
        };
        let connect = |folder: &Path| {
            let socket = folder.join("s.sock").display().to_string();
            let connect = "IO::Socket::UNIX->new(Peer => $ARGV[0]) or exit 1";
            let arguments = ["-MIO::Socket::UNIX", "-e", connect, &socket];
            run(&confinement, "/usr/bin/perl", &arguments)
        };
        assert!(connect(&named));
        assert!(!connect(&other));
    }
}
