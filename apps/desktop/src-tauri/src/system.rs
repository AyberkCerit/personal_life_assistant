//! What the scheduler needs to know about the computer: is the user away, is it on mains power.

pub trait SystemProbe: Send {
    fn idle_seconds(&self) -> u64;
    fn on_ac_power(&self) -> bool;
}

pub struct WindowsProbe;

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
