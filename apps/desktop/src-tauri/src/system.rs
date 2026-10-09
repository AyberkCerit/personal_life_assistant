//! What the scheduler needs to know about the computer: is the user away, is it on mains power.

pub trait SystemProbe: Send {
    fn idle_seconds(&self) -> u64;
    fn on_ac_power(&self) -> bool;
}

pub struct WindowsProbe;

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
        (GlobalMemoryStatusEx(&mut status) != 0).then(|| status.ullAvailPhys / (1024 * 1024))
    }
}

#[cfg(not(windows))]
pub fn available_ram_mb() -> Option<u64> {
    None
}

#[cfg(windows)]
impl SystemProbe for WindowsProbe {
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
impl SystemProbe for WindowsProbe {
    fn idle_seconds(&self) -> u64 {
        0
    }
    fn on_ac_power(&self) -> bool {
        true
    }
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
        assert!(available_ram_mb().is_some_and(|mb| mb > 0) || cfg!(not(windows)));
    }
}
