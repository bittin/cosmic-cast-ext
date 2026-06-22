// SPDX-License-Identifier: MPL-2.0

use std::env;
use std::fmt;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

const DBUS_MANAGER_NAME: &str = "org.gnome.NetworkDisplays.Manager";
const DBUS_MANAGER_PATH: &str = "/org/gnome/NetworkDisplays/Manager";
const FLATPAK_APP_ID: &str = "org.gnome.NetworkDisplays";
const GNOME_NETWORK_DISPLAYS: &str = "gnome-network-displays";
const GNOME_NETWORK_DISPLAYS_DAEMON: &str = "gnome-network-displays-daemon";
const MIRACAST_SCAN_ATTEMPTS: usize = 8;
const MIRACAST_SCAN_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Backend {
    GnomeNetworkDisplays,
    GnomeNetworkDisplaysFlatpak,
}

impl Backend {
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::GnomeNetworkDisplays => "GNOME Network Displays",
            Self::GnomeNetworkDisplaysFlatpak => "GNOME Network Displays (Flatpak)",
        }
    }

    #[must_use]
    pub const fn detail(&self) -> &'static str {
        match self {
            Self::GnomeNetworkDisplays => {
                "Uses the system GNOME Network Displays app; daemon controls may fall back to the Flathub build."
            }
            Self::GnomeNetworkDisplaysFlatpak => {
                "Uses the Flathub GNOME Network Displays Miracast sender."
            }
        }
    }

    fn launch_command(&self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::GnomeNetworkDisplays => (GNOME_NETWORK_DISPLAYS, &[]),
            Self::GnomeNetworkDisplaysFlatpak => ("flatpak", &["run", FLATPAK_APP_ID]),
        }
    }

    fn stop_command(&self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::GnomeNetworkDisplays => ("pkill", &["-x", GNOME_NETWORK_DISPLAYS]),
            Self::GnomeNetworkDisplaysFlatpak => ("flatpak", &["kill", FLATPAK_APP_ID]),
        }
    }

    async fn daemon_command(&self) -> Result<(&'static str, &'static [&'static str]), String> {
        match self {
            Self::GnomeNetworkDisplays if command_exists(GNOME_NETWORK_DISPLAYS_DAEMON) => {
                Ok((GNOME_NETWORK_DISPLAYS_DAEMON, &[]))
            }
            Self::GnomeNetworkDisplays if flatpak_app_installed().await => Ok((
                "flatpak",
                &[
                    "run",
                    "--command=gnome-network-displays-daemon",
                    FLATPAK_APP_ID,
                ],
            )),
            Self::GnomeNetworkDisplays => Err(
                "Neither the system GNOME Network Displays package nor the Flathub build exposes the DBus daemon used for Miracast controls"
                    .to_owned(),
            ),
            Self::GnomeNetworkDisplaysFlatpak => Ok((
                "flatpak",
                &[
                    "run",
                    "--command=gnome-network-displays-daemon",
                    FLATPAK_APP_ID,
                ],
            )),
        }
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum CheckStatus {
    Available,
    Missing,
    Error(String),
}

impl CheckStatus {
    #[must_use]
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }

    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Available => "Available".to_owned(),
            Self::Missing => "Not found".to_owned(),
            Self::Error(error) => format!("Unavailable: {error}"),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DisplayProtocol {
    WifiDisplayP2p,
    WifiDisplayMice,
    Chromecast,
    Other(u32),
}

impl DisplayProtocol {
    fn from_raw(raw: u32) -> Self {
        match raw {
            // GNOME Network Displays 0.99 has internal dummy protocols before real ones.
            1 | 3 => Self::WifiDisplayP2p,
            2 | 4 => Self::WifiDisplayMice,
            5 => Self::Chromecast,
            protocol => Self::Other(protocol),
        }
    }

    #[must_use]
    pub const fn is_miracast(&self) -> bool {
        matches!(self, Self::WifiDisplayP2p | Self::WifiDisplayMice)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DisplayState {
    Disconnected,
    EnsureFirewall,
    WaitP2p,
    WaitSocket,
    WaitStreaming,
    Streaming,
    Error,
    Other(u32),
}

impl DisplayState {
    fn from_raw(raw: u32) -> Self {
        match raw {
            0x0 => Self::Disconnected,
            0x50 => Self::EnsureFirewall,
            0x100 => Self::WaitP2p,
            0x110 => Self::WaitSocket,
            0x120 => Self::WaitStreaming,
            0x1000 => Self::Streaming,
            0x10000 => Self::Error,
            state => Self::Other(state),
        }
    }

    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Disconnected => "Ready",
            Self::EnsureFirewall => "Checking firewall",
            Self::WaitP2p => "Connecting Wi-Fi Direct",
            Self::WaitSocket => "Opening connection",
            Self::WaitStreaming => "Starting stream",
            Self::Streaming => "Streaming",
            Self::Error => "Error",
            Self::Other(_) => "Unknown state",
        }
    }

    #[must_use]
    pub const fn is_active(&self) -> bool {
        matches!(
            self,
            Self::EnsureFirewall
                | Self::WaitP2p
                | Self::WaitSocket
                | Self::WaitStreaming
                | Self::Streaming
        )
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CastDisplay {
    pub uuid: String,
    pub name: String,
    pub protocol: DisplayProtocol,
    pub state: DisplayState,
}

impl CastDisplay {
    #[must_use]
    pub fn stream_unit_name(&self) -> String {
        format!("gnome-network-displays-stream-{}.service", self.uuid)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RuntimeStatus {
    pub backend: Option<Backend>,
    pub running: bool,
    pub network_manager: CheckStatus,
    pub wifi_direct: CheckStatus,
    pub display_scan: CheckStatus,
    pub miracast_displays: Vec<CastDisplay>,
    pub chromecast_count: usize,
    pub wpa_cli: CheckStatus,
    pub flatpak: CheckStatus,
    pub notes: Vec<String>,
}

impl RuntimeStatus {
    #[must_use]
    pub fn summary(&self) -> String {
        if self.backend.is_none() {
            return "Install GNOME Network Displays to cast".to_owned();
        }

        if !self.wifi_direct.is_available() {
            return "Miracast unavailable; Chromecast discovery may still work".to_owned();
        }

        if let Some(display) = self
            .miracast_displays
            .iter()
            .find(|display| display.state.is_active())
        {
            return format!("{}: {}", display.name, display.state.label());
        }

        if !self.miracast_displays.is_empty() {
            return format!(
                "Found {} Miracast display{}",
                self.miracast_displays.len(),
                if self.miracast_displays.len() == 1 {
                    ""
                } else {
                    "s"
                }
            );
        }

        if self.chromecast_count > 0 {
            return format!(
                "No Miracast displays found; {} Chromecast device{} ignored",
                self.chromecast_count,
                if self.chromecast_count == 1 { "" } else { "s" }
            );
        }

        match (&self.backend, self.running) {
            (Some(backend), true) => format!("Casting helper is open through {backend}"),
            (Some(backend), false) => format!("Ready to cast with {backend}"),
            (None, _) => unreachable!("backend absence is handled above"),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BackendActionResult {
    pub status: RuntimeStatus,
    pub message: String,
}

pub async fn inspect() -> RuntimeStatus {
    let flatpak = if command_exists("flatpak") {
        if flatpak_app_installed().await {
            CheckStatus::Available
        } else {
            CheckStatus::Missing
        }
    } else {
        CheckStatus::Missing
    };

    let backend = if command_exists(GNOME_NETWORK_DISPLAYS) {
        Some(Backend::GnomeNetworkDisplays)
    } else if flatpak.is_available() {
        Some(Backend::GnomeNetworkDisplaysFlatpak)
    } else {
        None
    };

    let network_manager = check_network_manager().await;
    let wifi_direct = check_wifi_direct().await;
    let display_scan = match backend.as_ref() {
        Some(backend) => scan_displays(backend).await,
        None => DisplayScan::default(),
    };
    let running = backend.as_ref().is_some_and(is_backend_running);
    let wpa_cli = if command_exists("wpa_cli") {
        CheckStatus::Available
    } else {
        CheckStatus::Missing
    };

    let mut notes = Vec::new();

    if backend.is_none() {
        notes.push(
            "Install GNOME Network Displays, then use this applet to open the wireless display picker."
                .to_owned(),
        );
    }

    if !network_manager.is_available() {
        notes.push(
            "Miracast discovery needs NetworkManager and a Wi-Fi adapter with Wi-Fi Direct support."
                .to_owned(),
        );
    }

    if backend.is_some() && !wifi_direct.is_available() {
        notes.push(
            "Chromecast devices are discovered on the local network. Windows wireless display adapters use Miracast over Wi-Fi Direct, so they will not appear until the Wi-Fi Direct check passes."
                .to_owned(),
        );
    }

    if backend.is_some()
        && wifi_direct.is_available()
        && display_scan.status.is_available()
        && display_scan.miracast_displays.is_empty()
    {
        notes.push(
            "Wi-Fi Direct is available, but no Miracast sinks were reported during the scan. If debug logs only show P2P discovery starting or restarting, the adapter is not visible as a P2P peer."
                .to_owned(),
        );
    }

    if backend == Some(Backend::GnomeNetworkDisplays) && !display_scan.status.is_available() {
        notes.push(
            "The system GNOME Network Displays package can be launched, but this build does not expose the daemon API used by the applet for Miracast-only filtering."
                .to_owned(),
        );
    }

    if backend == Some(Backend::GnomeNetworkDisplays)
        && display_scan.status.is_available()
        && !command_exists(GNOME_NETWORK_DISPLAYS_DAEMON)
        && flatpak.is_available()
    {
        notes.push(
            "Opening uses the system GNOME Network Displays app; Miracast scanning and direct stream controls are using the Flathub daemon."
                .to_owned(),
        );
    }

    if display_scan.chromecast_count > 0 {
        notes.push(
            "Chromecast targets are intentionally ignored by this applet; they use a different protocol and are often high-latency for desktop mirroring."
                .to_owned(),
        );
    }

    RuntimeStatus {
        backend,
        running,
        network_manager,
        wifi_direct,
        display_scan: display_scan.status,
        miracast_displays: display_scan.miracast_displays,
        chromecast_count: display_scan.chromecast_count,
        wpa_cli,
        flatpak,
        notes,
    }
}

struct DisplayScan {
    status: CheckStatus,
    miracast_displays: Vec<CastDisplay>,
    chromecast_count: usize,
}

impl Default for DisplayScan {
    fn default() -> Self {
        Self {
            status: CheckStatus::Missing,
            miracast_displays: Vec::new(),
            chromecast_count: 0,
        }
    }
}

pub async fn open_picker(backend: Backend) -> BackendActionResult {
    let (program, args) = backend.launch_command();
    let message = match spawn_detached(program, args) {
        Ok(()) => "Wireless display picker opened".to_owned(),
        Err(error) => format!("Could not open wireless display picker: {error}"),
    };

    BackendActionResult {
        status: inspect().await,
        message,
    }
}

pub async fn start_stream(backend: Backend, display: CastDisplay) -> BackendActionResult {
    let message = match start_stream_unit(&backend, &display).await {
        Ok(unit_name) => format!("Started streaming to {} through {unit_name}", display.name),
        Err(error) => format!("Could not start streaming to {}: {error}", display.name),
    };

    BackendActionResult {
        status: inspect().await,
        message,
    }
}

pub async fn stop_stream(backend: Backend, display: CastDisplay) -> BackendActionResult {
    let unit_name = display.stream_unit_name();
    let message = match stop_stream_unit(&backend, &unit_name).await {
        Ok(()) => format!("Stopped streaming to {}", display.name),
        Err(error) => format!("Could not stop streaming to {}: {error}", display.name),
    };

    BackendActionResult {
        status: inspect().await,
        message,
    }
}

pub async fn stop_casting(backend: Backend) -> BackendActionResult {
    let (program, args) = backend.stop_command();
    let message = match Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
    {
        Ok(status) if status.success() => "Casting helper closed".to_owned(),
        Ok(_) => "No active casting helper was found".to_owned(),
        Err(error) => format!("Could not close casting helper: {error}"),
    };

    BackendActionResult {
        status: inspect().await,
        message,
    }
}

async fn start_stream_unit(backend: &Backend, display: &CastDisplay) -> Result<String, String> {
    ensure_manager_running(backend).await?;

    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            DBUS_MANAGER_NAME,
            "--object-path",
            DBUS_MANAGER_PATH,
            "--method",
            "org.gnome.NetworkDisplays.Manager.StartStream",
        ])
        .arg(&display.uuid)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| error.to_string())?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_gdbus_tuple_string(&stdout)
            .ok_or_else(|| "Stream started, but the daemon did not return a stream unit".to_owned())
    } else {
        Err(command_error_message(
            &output,
            "StartStream returned an error",
        ))
    }
}

async fn stop_stream_unit(backend: &Backend, unit_name: &str) -> Result<(), String> {
    ensure_manager_running(backend).await?;

    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            DBUS_MANAGER_NAME,
            "--object-path",
            DBUS_MANAGER_PATH,
            "--method",
            "org.gnome.NetworkDisplays.Manager.StopStream",
        ])
        .arg(unit_name)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| error.to_string())?;

    if output.status.success() {
        Ok(())
    } else {
        Err(command_error_message(
            &output,
            "StopStream returned an error",
        ))
    }
}

async fn scan_displays(backend: &Backend) -> DisplayScan {
    if !command_exists("gdbus") {
        return DisplayScan {
            status: CheckStatus::Missing,
            ..DisplayScan::default()
        };
    }

    if let Err(error) = ensure_manager_running(backend).await {
        return DisplayScan {
            status: CheckStatus::Error(error),
            ..DisplayScan::default()
        };
    }

    let mut last_scan = DisplayScan::default();

    for attempt in 0..MIRACAST_SCAN_ATTEMPTS {
        match query_manager_displays().await {
            Ok(displays) => {
                last_scan = displays_to_scan(displays);

                if !last_scan.miracast_displays.is_empty() {
                    return last_scan;
                }
            }
            Err(status) => {
                return DisplayScan {
                    status,
                    ..DisplayScan::default()
                };
            }
        }

        if attempt + 1 < MIRACAST_SCAN_ATTEMPTS {
            tokio::time::sleep(MIRACAST_SCAN_INTERVAL).await;
        }
    }

    last_scan
}

async fn query_manager_displays() -> Result<Vec<CastDisplay>, CheckStatus> {
    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            DBUS_MANAGER_NAME,
            "--object-path",
            DBUS_MANAGER_PATH,
            "--method",
            "org.freedesktop.DBus.Properties.Get",
            DBUS_MANAGER_NAME,
            "Displays",
        ])
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| CheckStatus::Error(error.to_string()))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_manager_displays(&stdout))
    } else {
        Err(command_error_status(
            &output,
            "Display scan returned an error",
        ))
    }
}

fn displays_to_scan(displays: Vec<CastDisplay>) -> DisplayScan {
    let chromecast_count = displays
        .iter()
        .filter(|display| matches!(display.protocol, DisplayProtocol::Chromecast))
        .count();
    let miracast_displays = displays
        .into_iter()
        .filter(|display| display.protocol.is_miracast())
        .collect();

    DisplayScan {
        status: CheckStatus::Available,
        miracast_displays,
        chromecast_count,
    }
}

async fn ensure_manager_running(backend: &Backend) -> Result<(), String> {
    if manager_has_owner().await? {
        return Ok(());
    }

    let (program, args) = backend.daemon_command().await?;
    spawn_detached(program, args)?;
    tokio::time::sleep(Duration::from_millis(900)).await;

    if manager_has_owner().await? {
        Ok(())
    } else {
        Err("GNOME Network Displays daemon did not appear on DBus".to_owned())
    }
}

async fn manager_has_owner() -> Result<bool, String> {
    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.DBus",
            "--object-path",
            "/org/freedesktop/DBus",
            "--method",
            "org.freedesktop.DBus.NameHasOwner",
            DBUS_MANAGER_NAME,
        ])
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| error.to_string())?;

    if !output.status.success() {
        return Err(command_error_message(
            &output,
            "Could not check GNOME Network Displays DBus owner",
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).contains("true"))
}

fn parse_manager_displays(raw: &str) -> Vec<CastDisplay> {
    raw.split("{'uuid':")
        .skip(1)
        .filter_map(parse_manager_display)
        .collect()
}

fn parse_manager_display(entry: &str) -> Option<CastDisplay> {
    let uuid = extract_gdbus_uuid(entry)?;
    let name = extract_gdbus_string(entry, "'display-name': <'")?;
    let state = extract_gdbus_u32(entry, "'state': <uint32 ")?;
    let protocol = extract_gdbus_u32(entry, "'protocol': <uint32 ")?;

    Some(CastDisplay {
        uuid,
        name,
        protocol: DisplayProtocol::from_raw(protocol),
        state: DisplayState::from_raw(state),
    })
}

fn extract_gdbus_uuid(entry: &str) -> Option<String> {
    let rest = entry.trim_start().strip_prefix("<'")?;
    let value = rest.split_once("'>")?.0;
    Some(value.replace("\\'", "'"))
}

fn extract_gdbus_string(entry: &str, prefix: &str) -> Option<String> {
    let rest = entry.split_once(prefix)?.1;
    let value = rest.split_once("'>")?.0;
    Some(value.replace("\\'", "'"))
}

fn extract_gdbus_u32(entry: &str, prefix: &str) -> Option<u32> {
    let rest = entry.split_once(prefix)?.1;
    rest.split_once('>')?.0.parse().ok()
}

fn parse_gdbus_tuple_string(raw: &str) -> Option<String> {
    let value = raw.split_once("('")?.1.split_once("',)")?.0;
    Some(value.replace("\\'", "'"))
}

fn spawn_detached(program: &str, args: &[&str]) -> Result<(), String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;

    tokio::spawn(async move {
        _ = child.wait().await;
    });

    Ok(())
}

async fn flatpak_app_installed() -> bool {
    Command::new("flatpak")
        .args(["info", FLATPAK_APP_ID])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .is_ok_and(|status| status.success())
}

async fn check_wifi_direct() -> CheckStatus {
    if !command_exists("nmcli") {
        return CheckStatus::Missing;
    }

    match Command::new("nmcli")
        .args(["-t", "-f", "DEVICE,TYPE,STATE", "device", "status"])
        .stdin(Stdio::null())
        .output()
        .await
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);

            if stdout.lines().any(nmcli_line_is_wifi_p2p) {
                return CheckStatus::Available;
            }

            if !stdout.lines().any(nmcli_line_is_wifi) {
                return CheckStatus::Missing;
            }

            match adapter_supports_wifi_direct().await {
                Some(Ok(true)) => CheckStatus::Error(
                    "Wi-Fi adapter supports P2P, but NetworkManager did not expose a Wi-Fi Direct device"
                        .to_owned(),
                ),
                Some(Ok(false)) => {
                    CheckStatus::Error("No Wi-Fi Direct support was reported by the adapter".to_owned())
                }
                Some(Err(error)) => CheckStatus::Error(error),
                None => CheckStatus::Error(
                    "No NetworkManager Wi-Fi Direct device was found; install iw for a hardware capability check"
                        .to_owned(),
                ),
            }
        }
        Ok(output) => command_error_status(&output, "nmcli device status returned an error"),
        Err(error) => CheckStatus::Error(error.to_string()),
    }
}

async fn check_network_manager() -> CheckStatus {
    if !command_exists("nmcli") {
        return CheckStatus::Missing;
    }

    match Command::new("nmcli")
        .args(["-t", "-f", "RUNNING", "general"])
        .stdin(Stdio::null())
        .output()
        .await
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.trim() == "running" {
                CheckStatus::Available
            } else {
                CheckStatus::Error("NetworkManager is not running".to_owned())
            }
        }
        Ok(output) => command_error_status(&output, "nmcli returned an error"),
        Err(error) => CheckStatus::Error(error.to_string()),
    }
}

async fn adapter_supports_wifi_direct() -> Option<Result<bool, String>> {
    if !command_exists("iw") {
        return None;
    }

    let output = match Command::new("iw")
        .arg("list")
        .stdin(Stdio::null())
        .output()
        .await
    {
        Ok(output) => output,
        Err(error) => return Some(Err(error.to_string())),
    };

    if !output.status.success() {
        return Some(Err(command_error_message(
            &output,
            "iw list returned an error",
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Some(Ok(stdout.contains("P2P-client")
        || stdout.contains("P2P-GO")
        || stdout.contains("P2P-device")))
}

fn nmcli_line_is_wifi_p2p(line: &str) -> bool {
    nmcli_device_type(line).is_some_and(|device_type| device_type == "wifi-p2p")
}

fn nmcli_line_is_wifi(line: &str) -> bool {
    nmcli_device_type(line).is_some_and(|device_type| device_type == "wifi")
}

fn nmcli_device_type(line: &str) -> Option<&str> {
    line.split(':').nth(1)
}

fn command_error_status(output: &std::process::Output, fallback: &str) -> CheckStatus {
    CheckStatus::Error(command_error_message(output, fallback))
}

fn command_error_message(output: &std::process::Output, fallback: &str) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if stderr.is_empty() {
        fallback.to_owned()
    } else {
        stderr
    }
}

fn is_backend_running(backend: &Backend) -> bool {
    match backend {
        Backend::GnomeNetworkDisplays => command_status("pgrep", ["-x", GNOME_NETWORK_DISPLAYS]),
        Backend::GnomeNetworkDisplaysFlatpak => flatpak_app_running(),
    }
}

fn flatpak_app_running() -> bool {
    let Ok(output) = std::process::Command::new("flatpak")
        .args(["ps", "--columns=application"])
        .stdin(Stdio::null())
        .output()
    else {
        return false;
    };

    output.status.success()
        && String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| line.trim() == FLATPAK_APP_ID)
}

fn command_status<const N: usize>(program: &str, args: [&str; N]) -> bool {
    std::process::Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn command_exists(program: &str) -> bool {
    let program = Path::new(program);

    if program.components().count() > 1 {
        return is_executable_file(program);
    }

    env::var_os("PATH").is_some_and(|paths| {
        env::split_paths(&paths).any(|path| is_executable_file(&path.join(program)))
    })
}

fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::{DisplayProtocol, DisplayState, parse_gdbus_tuple_string, parse_manager_displays};

    #[test]
    fn parses_gdbus_display_protocols() {
        let displays = parse_manager_displays(
            "(<[{'uuid': <'a'>, 'display-name': <'Office Adapter'>, 'priority': <uint32 50>, 'state': <uint32 0>, 'protocol': <uint32 3>}, {'uuid': <'b'>, 'display-name': <'Room TV - Chromecast'>, 'priority': <uint32 50>, 'state': <uint32 0>, 'protocol': <uint32 5>}]>,)",
        );

        assert_eq!(displays.len(), 2);
        assert_eq!(displays[0].uuid, "a");
        assert_eq!(displays[0].name, "Office Adapter");
        assert_eq!(displays[0].protocol, DisplayProtocol::WifiDisplayP2p);
        assert_eq!(displays[0].state, DisplayState::Disconnected);
        assert_eq!(displays[1].protocol, DisplayProtocol::Chromecast);
    }

    #[test]
    fn treats_older_dbus_protocol_ids_as_miracast() {
        let displays = parse_manager_displays(
            "(<[{'uuid': <'a'>, 'display-name': <'MICE Display'>, 'priority': <uint32 50>, 'state': <uint32 0>, 'protocol': <uint32 2>}]>,)",
        );

        assert_eq!(displays.len(), 1);
        assert!(displays[0].protocol.is_miracast());
    }

    #[test]
    fn parses_start_stream_unit() {
        assert_eq!(
            parse_gdbus_tuple_string("('gnome-network-displays-stream-a.service',)\n"),
            Some("gnome-network-displays-stream-a.service".to_owned())
        );
    }
}
