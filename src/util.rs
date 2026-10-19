// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::Write;
use std::path::Path;
use std::process::{Command, ExitStatus};

/// write through a temp file in the same directory, so a crash cannot leave
/// a half-written file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temp = parent.join(format!(".{name}.tmp"));
    let mut file = std::fs::File::create(&temp)?;
    file.write_all(bytes)?;
    drop(file);
    std::fs::rename(&temp, path)?;
    Ok(())
}

/// run a command, keeping its output in `log` for when it fails.
pub fn run_logged(command: &mut Command, log: &Path) -> anyhow::Result<ExitStatus> {
    let output = command.output()?;
    if let Some(parent) = log.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut text = output.stdout;
    text.extend_from_slice(&output.stderr);
    let _ = std::fs::write(log, text);
    Ok(output.status)
}

/// `300 MB` style sizes for progress lines.
pub fn format_size(bytes: u64) -> String {
    for (unit, scale) in [("GB", 1u64 << 30), ("MB", 1 << 20), ("KB", 1 << 10)] {
        if bytes >= scale {
            return format!("{:.1} {unit}", bytes as f64 / scale as f64);
        }
    }
    format!("{bytes} B")
}
