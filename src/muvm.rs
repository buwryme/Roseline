// SPDX-License-Identifier: GPL-3.0-or-later

//! running a runner built for another architecture, through muvm.
//!
//! Rosemary ships x86_64 builds only. On an aarch64 host the x86_64 runner
//! runs inside muvm's microVM, where FEX translates it and the pages are 4K
//! even when the host's are 16K (Asahi Linux).

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, bail};

use crate::config::Paths;

/// raises the VM's overcommit before the guest starts: Rosemary reserves a
/// very large address space, and with the VM's heuristic default and no swap
/// every fork it makes (helpers, the menu bar, `open`) fails with ENOMEM.
const PRE_SCRIPT: &str = "#!/bin/sh\nset -eu\necho 1 > /proc/sys/vm/overcommit_memory\n";

/// the architecture a runner was built for, from its `Rosemary/bin/rosemary`.
pub fn runner_arch(runner_dir: &Path) -> Option<&'static str> {
    binary_arch(&runner_dir.join("Rosemary/bin/rosemary"))
}

/// `x86_64` or `aarch64`, from an ELF header's machine field.
pub fn binary_arch(path: &Path) -> Option<&'static str> {
    let mut header = [0u8; 20];
    let mut file = std::fs::File::open(path).ok()?;
    std::io::Read::read_exact(&mut file, &mut header).ok()?;
    elf_arch(&header)
}

fn elf_arch(header: &[u8]) -> Option<&'static str> {
    if header.len() < 20 || &header[..4] != b"\x7fELF" || header[4] != 2 || header[5] != 1 {
        return None;
    }
    match u16::from_le_bytes([header[18], header[19]]) {
        0x3e => Some("x86_64"),
        0xb7 => Some("aarch64"),
        _ => None,
    }
}

/// whether a runner of this architecture has to go through the VM.
pub fn needed_for(arch: &str) -> bool {
    arch != std::env::consts::ARCH
}

/// whether this runner has to go through muvm; an error when it has to and
/// cannot.
pub fn route(runner_dir: &Path, enabled: bool) -> Result<bool> {
    let Some(arch) = runner_arch(runner_dir) else {
        return Ok(false);
    };
    if !needed_for(arch) {
        return Ok(false);
    }
    check_support(
        arch,
        std::env::consts::ARCH,
        enabled,
        crate::rpc::sandboxed(),
    )?;
    if !available() {
        bail!(
            "this Rosemary runner is built for {arch}, which this {} computer runs through muvm; {}",
            std::env::consts::ARCH,
            missing_hint()
        );
    }
    if vm_running() {
        bail!(
            "another muvm app is running (Steam, for one); close it first, as Roblox needs a VM of its own"
        );
    }
    Ok(true)
}

fn check_support(arch: &str, host: &str, enabled: bool, sandboxed: bool) -> Result<()> {
    if host != "aarch64" || arch != "x86_64" {
        bail!("a {arch} Rosemary runner cannot run on this {host} computer");
    }
    if !enabled {
        bail!(
            "ARM64 compatibility is disabled; enable it in Advanced settings or install a native ARM64 Rosemary runner"
        );
    }
    if sandboxed {
        bail!(
            "muvm cannot run inside the Flatpak sandbox; install Roséline's native package instead"
        );
    }
    Ok(())
}

/// whether `muvm` is on the PATH.
pub fn available() -> bool {
    find("muvm").is_some()
}

fn find(name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|candidate| {
            candidate
                .metadata()
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
}

/// whether a muvm VM is already up. A second `muvm` joins it and returns at
/// once, so the game would look like it exited, and the VM never ran the
/// overcommit script.
pub fn vm_running() -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    entries.flatten().any(|entry| {
        let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else {
            return false;
        };
        let program = cmdline.split(|byte| *byte == 0).next().unwrap_or_default();
        Path::new(OsStr::from_bytes(program)).file_name() == Some(OsStr::new("muvm"))
    })
}

/// how to tell the user what is missing, by host.
pub fn missing_hint() -> &'static str {
    if crate::rpc::sandboxed() {
        "muvm cannot run inside the Flatpak sandbox; install Roséline's native package instead"
    } else {
        "install muvm and FEX first (Fedora Asahi Remix: sudo dnf install muvm fex-emu)"
    }
}

/// `binary args…` inside the VM, with `environment` set, `directory` as the
/// working directory, and everything it prints written to `log`. muvm does not
/// carry the host's environment or output across, so both are set up inside.
pub fn command(
    binary: &Path,
    arguments: &[impl AsRef<OsStr>],
    environment: &[(String, String)],
    directory: &Path,
    log: &Path,
    memory: crate::settings::VmMemory,
) -> Result<Command> {
    let mut command = Command::new("muvm");
    if let Some(mib) = memory.mib() {
        command.arg(format!("--mem={mib}"));
    }
    command.arg("-x").arg(pre_script()?);
    for (name, value) in environment {
        command.arg("-e").arg(format!("{name}={value}"));
    }
    command
        .arg("--")
        .arg("/bin/sh")
        .arg("-c")
        .arg(r#"exec >>"$2" 2>&1; cd "$1" || exit; shift 2; exec "$@""#)
        .arg("sh")
        .arg(directory)
        .arg(log)
        .arg(binary)
        .args(arguments);
    Ok(command)
}

fn pre_script() -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = Paths::get().data.join("muvm-overcommit.sh");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(PRE_SCRIPT) {
        crate::util::write_atomic(&path, PRE_SCRIPT.as_bytes())?;
    }
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(machine: u16) -> Vec<u8> {
        let mut bytes = b"\x7fELF\x02\x01\x01".to_vec();
        bytes.resize(18, 0);
        bytes.extend_from_slice(&machine.to_le_bytes());
        bytes
    }

    #[test]
    fn elf_machines_name_the_architecture() {
        assert_eq!(elf_arch(&header(0x3e)), Some("x86_64"));
        assert_eq!(elf_arch(&header(0xb7)), Some("aarch64"));
        assert_eq!(elf_arch(&header(0x28)), None);
        assert_eq!(elf_arch(b"#!/bin/sh\nexec something\n"), None);
    }

    #[test]
    fn only_a_foreign_runner_needs_the_vm() {
        assert!(!needed_for(std::env::consts::ARCH));
        assert!(needed_for(match std::env::consts::ARCH {
            "aarch64" => "x86_64",
            _ => "aarch64",
        }));
    }

    #[test]
    fn this_test_binary_reads_as_the_host() {
        let exe = std::env::current_exe().unwrap();
        assert_eq!(binary_arch(&exe), Some(std::env::consts::ARCH));
    }

    #[test]
    fn unsupported_disabled_and_sandboxed_routes_fail_early() {
        assert!(check_support("x86_64", "aarch64", true, false).is_ok());
        assert!(
            check_support("x86_64", "aarch64", false, false)
                .unwrap_err()
                .to_string()
                .contains("disabled")
        );
        assert!(
            check_support("x86_64", "aarch64", true, true)
                .unwrap_err()
                .to_string()
                .contains("Flatpak")
        );
        assert!(check_support("aarch64", "x86_64", true, false).is_err());
        let mut invalid = header(0x3e);
        invalid[4] = 1;
        assert_eq!(elf_arch(&invalid), None);
    }

    #[test]
    #[ignore = "starts a real muvm VM; run alone on an idle ARM64 host"]
    fn vm_preserves_arguments_environment_logs_and_exit_status() {
        assert!(!vm_running(), "close the existing VM before this test");
        let directory =
            std::env::temp_dir().join(format!("roseline vm 'probe {}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let log = directory.join("guest $log.txt");
        std::fs::write(&log, "existing log\n").unwrap();
        let arguments = [
            "-c",
            "printf '%s\\n' \"$PROBE\" \"$1\" \"$PWD\"; exit 7",
            "sh",
            "literal $value; 'quote'",
        ];
        let environment = [("PROBE".to_string(), "spaces $dollar 'quote".to_string())];
        let output = command(
            Path::new("/bin/sh"),
            &arguments,
            &environment,
            &directory,
            &log,
            crate::settings::VmMemory::Auto,
        )
        .unwrap()
        .output()
        .unwrap();
        assert_eq!(
            output.status.code(),
            Some(7),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            format!(
                "existing log\nspaces $dollar 'quote\nliteral $value; 'quote'\n{}\n",
                directory.display()
            )
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
