//! What the scheduler needs to know about the computer: is the user away, is it on mains power.

pub trait SystemProbe: Send {
    fn idle_seconds(&self) -> u64;
    fn on_ac_power(&self) -> bool;
}

/// The desktop probe. On Windows it asks the OS; elsewhere see the `cfg(not(windows))` impl.
pub struct DesktopProbe;

/// Below this much free memory the embedding server goes before the chat model answers; above it,
/// keeping it saves the next question a cold start (final review I6).
pub const RELEASE_BELOW_MB: u64 = 4096;

/// Whether free memory is short enough to stop a model that is not needed this moment. Unknown counts
/// as short.
pub fn should_release(available_mb: Option<u64>) -> bool {
    available_mb.is_none_or(|mb| mb < RELEASE_BELOW_MB)
}

/// Free physical memory in MiB.
#[cfg(windows)]
pub fn available_ram_mb() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: plain Win32 call with a correctly sized struct.
    unsafe {
        let mut status: MEMORYSTATUSEX = std::mem::zeroed();
        status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        (GlobalMemoryStatusEx(&mut status) != 0).then_some(status.ullAvailPhys / (1024 * 1024))
    }
}

/// Free physical memory in MiB: `MemAvailable` from `/proc/meminfo`.
#[cfg(target_os = "linux")]
pub fn available_ram_mb() -> Option<u64> {
    std::fs::read_to_string("/proc/meminfo").ok().as_deref().and_then(parse_meminfo_available_mb)
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn available_ram_mb() -> Option<u64> {
    None
}

/// `MemAvailable` of a `/proc/meminfo` text, in MiB. `None` when the line is missing (kernels
/// before 3.14) or unreadable.
pub fn parse_meminfo_available_mb(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find_map(|l| l.strip_prefix("MemAvailable:"))?;
    let mut parts = line.split_whitespace();
    let kib: u64 = parts.next()?.parse().ok()?;
    matches!(parts.next(), Some("kB") | None).then_some(kib / 1024)
}

/// Whether the computer runs on external power, from `/sys/class/power_supply` entries given as
/// (`<supply>/<file>`, trimmed contents) pairs for the `type` and `online` files. Any `Mains`
/// supply with `online` = 1 means yes; `Mains` supplies all offline means battery. No `Mains`
/// supply at all (a desktop PC, or one whose only battery is a wireless mouse's) counts as AC
/// power, as Windows reports for a desktop without a battery.
pub fn parse_power_supply_online(entries: &[(String, String)]) -> bool {
    let mains: Vec<&str> = entries.iter().filter(|(_, v)| v == "Mains").filter_map(|(k, _)| k.strip_suffix("/type")).collect();
    let online = |supply: &str| entries.iter().any(|(k, v)| v == "1" && k.strip_suffix("/online") == Some(supply));
    mains.is_empty() || mains.into_iter().any(online)
}

/// When the PLA window last saw the user (key, pointer or wheel), sent by the UI as `user_active`.
/// Off Windows this is what idle time is measured from; Windows asks the OS and ignores it.
static LAST_ACTIVE: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// The UI saw the user: off Windows, idle time starts again from now.
pub fn mark_user_active() {
    *LAST_ACTIVE.lock().expect("last active lock") = Some(std::time::Instant::now());
}

#[cfg(windows)]
impl SystemProbe for DesktopProbe {
    fn idle_seconds(&self) -> u64 {
        use windows_sys::Win32::System::SystemInformation::GetTickCount;
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
        let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
        // SAFETY: plain Win32 calls with a correctly sized struct.
        unsafe {
            if GetLastInputInfo(&mut info) == 0 {
                return 0;
            }
            u64::from(GetTickCount().wrapping_sub(info.dwTime)) / 1000
        }
    }

    fn on_ac_power(&self) -> bool {
        use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
        // SAFETY: plain Win32 call with a valid out pointer.
        unsafe {
            let mut status: SYSTEM_POWER_STATUS = std::mem::zeroed();
            // ACLineStatus: 0 = battery, 1 = mains, 255 = unknown (desktops without a battery report 1).
            GetSystemPowerStatus(&mut status) != 0 && status.ACLineStatus != 0
        }
    }
}

#[cfg(not(windows))]
impl SystemProbe for DesktopProbe {
    /// Seconds since the user last touched the PLA window (`mark_user_active`), counted from the
    /// first call when the UI has not reported yet. Input to other windows cannot be read portably
    /// here (X11 and Wayland differ, Wayland hides it), so unlike Windows this is not system-wide
    /// idle time: someone working in another program for five minutes counts as away, and the
    /// daily maintenance may run then (on mains power only).
    fn idle_seconds(&self) -> u64 {
        let mut last = LAST_ACTIVE.lock().expect("last active lock");
        last.get_or_insert_with(std::time::Instant::now).elapsed().as_secs()
    }

    /// Linux: `/sys/class/power_supply` (see `parse_power_supply_online`); elsewhere assumed.
    fn on_ac_power(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            parse_power_supply_online(&read_power_supplies())
        }
        #[cfg(not(target_os = "linux"))]
        {
            true
        }
    }
}

/// The `type` and `online` files of every supply under `/sys/class/power_supply`; empty when the
/// folder is missing.
#[cfg(target_os = "linux")]
fn read_power_supplies() -> Vec<(String, String)> {
    let mut entries = Vec::new();
    let Ok(dir) = std::fs::read_dir("/sys/class/power_supply") else { return entries };
    for supply in dir.flatten() {
        let name = supply.file_name().to_string_lossy().into_owned();
        for file in ["type", "online"] {
            if let Ok(text) = std::fs::read_to_string(supply.path().join(file)) {
                entries.push((format!("{name}/{file}"), text.trim().to_owned()));
            }
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_goes_only_when_memory_is_short() {
        // final review I6: every question paid a cold start for the embedding server
        assert!(should_release(Some(2048)));
        assert!(!should_release(Some(12_000)));
        assert!(should_release(None), "unknown counts as short");
        assert!(available_ram_mb().is_some_and(|mb| mb > 0) || cfg!(not(any(windows, target_os = "linux"))));
    }

    #[test]
    fn meminfo_available_is_read_in_mib() {
        let m = "MemTotal:       16000000 kB\nMemFree:  100 kB\nMemAvailable:    4194304 kB\n";
        assert_eq!(parse_meminfo_available_mb(m), Some(4096));
        assert_eq!(parse_meminfo_available_mb("MemTotal: 16000000 kB\n"), None, "an old kernel without MemAvailable");
    }

    #[test]
    fn any_online_mains_supply_means_ac_power() {
        let e = vec![("BAT0/type".into(), "Battery".into()), ("AC/type".into(), "Mains".into()), ("AC/online".into(), "1".into())];
        assert!(parse_power_supply_online(&e));
        let unplugged = vec![("BAT0/type".into(), "Battery".into()), ("AC/type".into(), "Mains".into()), ("AC/online".into(), "0".into())];
        assert!(!parse_power_supply_online(&unplugged));
    }

    #[test]
    fn no_supply_information_counts_as_ac_power() {
        assert!(parse_power_supply_online(&[])); // desktop PC without /sys/class/power_supply
        // a desktop whose only supply is a wireless mouse's battery
        assert!(parse_power_supply_online(&[("hidpp_battery_0/type".into(), "Battery".into())]));
    }

    #[cfg(not(windows))]
    #[test]
    fn touching_the_window_restarts_idle_time() {
        mark_user_active();
        assert_eq!(DesktopProbe.idle_seconds(), 0);
    }
}
