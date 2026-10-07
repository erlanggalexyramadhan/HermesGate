//! ADB integration boundary.
//!
//! The single module allowed to locate, spawn and parse `adb`. Other modules
//! ask this boundary for devices to target instead of shelling out themselves.
//!
//! HermesGate reuses the official `adb` binary (bundled in `src-tauri/binaries/`)
//! as the proven ADB implementation, so no ADB protocol code lives here — this
//! module only locates, invokes and parses it.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// One `adb` invocation may not take longer than this: a wedged server must
/// not hang a command the UI is waiting on.
const RUN_TIMEOUT: Duration = Duration::from_secs(10);

/// Where a usable `adb` executable came from. Surfaced in the UI for
/// diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdbSource {
    /// Explicit `HERMESGATE_ADB` override.
    Env,
    /// Shipped with HermesGate.
    Bundled,
    /// Found on `PATH`.
    Path,
    /// Default Android SDK install location.
    Sdk,
}

impl AdbSource {
    pub fn as_str(self) -> &'static str {
        match self {
            AdbSource::Env => "env",
            AdbSource::Bundled => "bundled",
            AdbSource::Path => "path",
            AdbSource::Sdk => "sdk",
        }
    }
}

/// A resolved `adb` executable and where it came from.
#[derive(Clone, Debug)]
pub struct Adb {
    pub path: PathBuf,
    pub source: AdbSource,
}

/// One line of `adb devices -l`, before device lifecycle is interpreted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawDevice {
    pub serial: String,
    /// Verbatim `adb` state (`device`, `unauthorized`, `offline`, …).
    pub state: String,
    pub model: Option<String>,
}

/// Metadata read from an attached device with `getprop`. `None` where the
/// device did not report a value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceProps {
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub android_version: Option<String>,
}

/// Locate `adb`.
///
/// Order: `HERMESGATE_ADB` (authoritative, fails loud when wrong) → bundled
/// with the app → development tree → `PATH` → default SDK locations. When
/// nothing is found the returned error is shown to the user as-is.
pub fn resolve() -> Result<Adb, String> {
    if let Some(overridden) = std::env::var_os("HERMESGATE_ADB") {
        let path = PathBuf::from(overridden);
        return if path.is_file() {
            Ok(Adb {
                path,
                source: AdbSource::Env,
            })
        } else {
            Err(format!(
                "HERMESGATE_ADB points to a missing file: {}",
                path.display()
            ))
        };
    }
    // Installed layout: the bundle copies `binaries/` next to the executable.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for base in [dir.join("binaries"), dir.to_path_buf()] {
                let path = base.join(executable_name());
                if path.is_file() {
                    return Ok(Adb {
                        path,
                        source: AdbSource::Bundled,
                    });
                }
            }
        }
    }
    // Development layout (`tauri dev` runs the binary from target/debug).
    let dev = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(executable_name());
    if dev.is_file() {
        return Ok(Adb {
            path: dev,
            source: AdbSource::Bundled,
        });
    }
    if let Some(path) = find_on_path() {
        return Ok(Adb {
            path,
            source: AdbSource::Path,
        });
    }
    for sdk in sdk_roots() {
        let path = sdk.join("platform-tools").join(executable_name());
        if path.is_file() {
            return Ok(Adb {
                path,
                source: AdbSource::Sdk,
            });
        }
    }
    Err("adb not found: install Android platform-tools or set HERMESGATE_ADB".to_string())
}

/// Run one `adb` command and return its standard output.
fn run(adb: &Path, args: &[&str]) -> Result<String, String> {
    let mut child = Command::new(adb)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start adb: {error}"))?;
    let mut stdout = child.stdout.take().ok_or("adb stdout unavailable")?;
    let mut stderr = child.stderr.take().ok_or("adb stderr unavailable")?;
    let stdout_reader = std::thread::spawn(move || read_all(&mut stdout));
    let stderr_reader = std::thread::spawn(move || read_all(&mut stderr));

    let deadline = Instant::now() + RUN_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("adb timed out after {}s", RUN_TIMEOUT.as_secs()));
            }
            Err(error) => break Err(format!("adb wait failed: {error}")),
        }
    };

    let out = stdout_reader
        .join()
        .map_err(|_| "adb stdout reader panicked".to_string())?;
    let err = stderr_reader
        .join()
        .map_err(|_| "adb stderr reader panicked".to_string())?;
    match status {
        Ok(status) if status.success() => Ok(String::from_utf8_lossy(&out).into_owned()),
        Ok(status) => Err(format!(
            "adb {} failed (code {:?}): {}",
            args.join(" "),
            status.code(),
            last_line(&String::from_utf8_lossy(&err))
        )),
        Err(error) => Err(error),
    }
}

/// Enumerate attached devices (`adb devices -l`).
pub fn list_devices(adb: &Path) -> Result<Vec<RawDevice>, String> {
    Ok(parse_devices(&run(adb, &["devices", "-l"])?))
}

/// Read manufacturer, model and Android version from an attached device.
pub fn device_props(adb: &Path, serial: &str) -> Result<DeviceProps, String> {
    Ok(parse_props(&run(adb, &["-s", serial, "shell", "getprop"])?))
}

fn parse_devices(output: &str) -> Vec<RawDevice> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty() && !line.starts_with('*') && !line.starts_with("List of devices")
        })
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let serial = fields.next()?.to_string();
            let state = fields.next()?.to_string();
            let model = fields
                .find_map(|field| field.strip_prefix("model:"))
                .map(str::to_string);
            Some(RawDevice {
                serial,
                state,
                model,
            })
        })
        .collect()
}

fn parse_props(output: &str) -> DeviceProps {
    let mut props = DeviceProps::default();
    for line in output.lines() {
        let Some(rest) = line.trim().strip_prefix('[') else {
            continue;
        };
        let Some((key, value)) = rest.split_once("]:[") else {
            continue;
        };
        let value = value.trim_end_matches(']');
        if value.is_empty() {
            continue;
        }
        match key {
            "ro.product.manufacturer" => props.manufacturer = Some(value.to_string()),
            "ro.product.model" => props.model = Some(value.to_string()),
            "ro.build.version.release" => props.android_version = Some(value.to_string()),
            _ => {}
        }
    }
    props
}

fn executable_name() -> &'static str {
    if cfg!(windows) {
        "adb.exe"
    } else {
        "adb"
    }
}

fn find_on_path() -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(executable_name()))
        .find(|path| path.is_file())
}

fn sdk_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        roots.push(PathBuf::from(local).join("Android").join("Sdk"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        roots.push(home.join("Android").join("Sdk"));
        roots.push(home.join("Library").join("Android").join("sdk"));
    }
    roots
}

fn read_all(reader: &mut impl Read) -> Vec<u8> {
    let mut buffer = Vec::new();
    let _ = reader.read_to_end(&mut buffer);
    buffer
}

fn last_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or("no output")
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEVICES_OUT: &str = "List of devices attached\n\
        R5CT30ABCXY               device product:beyond1 model:SM-G973F device:beyond1 transport_id:1\n\
        1A2B3C4D                  unauthorized transport_id:2\n\
        0123456789ABCDEF          offline\n\
        * daemon started successfully\n";

    const GETPROP_OUT: &str = "[ro.product.manufacturer]:[samsung]\n\
        [ro.product.model]:[SM-G973F]\n\
        [ro.build.version.release]:[14]\n\
        [ro.product.name]:[beyond1]\n";

    #[test]
    fn parses_device_states_and_models() {
        let devices = parse_devices(DEVICES_OUT);
        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].serial, "R5CT30ABCXY");
        assert_eq!(devices[0].state, "device");
        assert_eq!(devices[0].model.as_deref(), Some("SM-G973F"));
        assert_eq!(devices[1].state, "unauthorized");
        assert_eq!(devices[1].model, None);
        assert_eq!(devices[2].state, "offline");
    }

    #[test]
    fn parses_getprop_lines() {
        let props = parse_props(GETPROP_OUT);
        assert_eq!(props.manufacturer.as_deref(), Some("samsung"));
        assert_eq!(props.model.as_deref(), Some("SM-G973F"));
        assert_eq!(props.android_version.as_deref(), Some("14"));
    }
}
