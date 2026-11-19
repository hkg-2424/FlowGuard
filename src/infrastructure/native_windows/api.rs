//! Safe Rust wrappers around native Windows Win32 APIs for activity and idle observation.
//!
//! Utilizes:
//! - `GetForegroundWindow`: retrieves handle to foreground window.
//! - `GetWindowTextW`: retrieves title of foreground window.
//! - `GetWindowThreadProcessId`: retrieves process ID owning foreground window.
//! - `OpenProcess` + `QueryFullProcessImageNameW`: retrieves application executable path.
//! - `GetLastInputInfo` + `GetTickCount`: retrieves user idle/AFK duration.

#[cfg(target_os = "windows")]
#[allow(clippy::upper_case_acronyms)]
mod win32 {
    use std::ffi::c_void;

    pub type HWND = *mut c_void;
    pub type HANDLE = *mut c_void;
    pub type DWORD = u32;
    pub type BOOL = i32;
    pub type WCHAR = u16;
    pub type UINT = u32;

    pub const PROCESS_QUERY_LIMITED_INFORMATION: DWORD = 0x1000;
    pub const FALSE: BOOL = 0;

    #[repr(C)]
    #[derive(Debug, Copy, Clone)]
    #[allow(non_snake_case)]
    pub struct LASTINPUTINFO {
        pub cbSize: UINT,
        pub dwTime: DWORD,
    }

    #[link(name = "user32")]
    extern "system" {
        pub fn GetForegroundWindow() -> HWND;
        pub fn GetWindowTextW(hWnd: HWND, lpString: *mut WCHAR, nMaxCount: i32) -> i32;
        pub fn GetWindowThreadProcessId(hWnd: HWND, lpdwProcessId: *mut DWORD) -> DWORD;
        pub fn GetLastInputInfo(plii: *mut LASTINPUTINFO) -> BOOL;
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn OpenProcess(
            dwDesiredAccess: DWORD,
            bInheritHandle: BOOL,
            dwProcessId: DWORD,
        ) -> HANDLE;
        pub fn QueryFullProcessImageNameW(
            hProcess: HANDLE,
            dwFlags: DWORD,
            lpExeName: *mut WCHAR,
            lpdwSize: *mut DWORD,
        ) -> BOOL;
        pub fn CloseHandle(hObject: HANDLE) -> BOOL;
        pub fn GetTickCount() -> DWORD;
    }
}

/// Retrieves the foreground window's application name and title atomically in a single pass.
///
/// Returns `(application, window_title)`:
/// - Application name is extracted from the executable path (e.g. `Code.exe` -> `code.exe`)
///   and sanitized to lowercase without path or control characters.
/// - Window title is sanitized to remove control characters and normalized.
#[cfg(target_os = "windows")]
pub fn get_foreground_window_info() -> (Option<String>, Option<String>) {
    unsafe {
        let hwnd = win32::GetForegroundWindow();
        if hwnd.is_null() {
            return (None, None);
        }

        // 1. Window title
        let mut title_buf = [0u16; 512];
        let title_len = win32::GetWindowTextW(hwnd, title_buf.as_mut_ptr(), title_buf.len() as i32);
        let title = if title_len > 0 {
            let raw = String::from_utf16_lossy(&title_buf[..title_len as usize]);
            crate::infrastructure::activitywatch::mapper::sanitize_title(&raw)
        } else {
            None
        };

        // 2. Process ID
        let mut pid: win32::DWORD = 0;
        win32::GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return (None, title);
        }

        // 3. Process image name
        let handle =
            win32::OpenProcess(win32::PROCESS_QUERY_LIMITED_INFORMATION, win32::FALSE, pid);
        if handle.is_null() {
            return (None, title);
        }

        let mut path_buf = [0u16; 1024];
        let mut size: win32::DWORD = path_buf.len() as win32::DWORD;
        let success =
            win32::QueryFullProcessImageNameW(handle, 0, path_buf.as_mut_ptr(), &mut size);
        win32::CloseHandle(handle);

        let app = if success != 0 && size > 0 {
            let full_path = String::from_utf16_lossy(&path_buf[..size as usize]);
            crate::infrastructure::activitywatch::mapper::sanitize_app_name(&full_path)
        } else {
            None
        };

        (app, title)
    }
}

/// Queries elapsed user idle duration in milliseconds using `GetLastInputInfo` and `GetTickCount`.
///
/// Correctly accounts for 32-bit tick count wrapping via wrapping subtraction.
#[cfg(target_os = "windows")]
pub fn get_idle_duration_ms() -> Option<u64> {
    unsafe {
        let mut lii = win32::LASTINPUTINFO {
            cbSize: std::mem::size_of::<win32::LASTINPUTINFO>() as win32::UINT,
            dwTime: 0,
        };
        let ok = win32::GetLastInputInfo(&mut lii);
        if ok == 0 {
            return None;
        }

        let now_tick = win32::GetTickCount();
        let elapsed_ms = now_tick.wrapping_sub(lii.dwTime);
        Some(elapsed_ms as u64)
    }
}

/// Determines user idle state based on elapsed input milliseconds and configured timeout in seconds.
#[cfg(target_os = "windows")]
pub fn get_idle_state(afk_timeout_secs: u64) -> (crate::domain::IdleState, Option<u64>) {
    match get_idle_duration_ms() {
        Some(ms) => {
            let secs = ms / 1000;
            if secs >= afk_timeout_secs {
                (crate::domain::IdleState::Afk, Some(secs))
            } else {
                (crate::domain::IdleState::Active, Some(secs))
            }
        }
        None => (crate::domain::IdleState::Unknown, None),
    }
}

// ----------------------------------------------------------------------------
// Non-Windows fallbacks (for compiling on Linux/macOS in CI/mock environments)
// ----------------------------------------------------------------------------

#[cfg(not(target_os = "windows"))]
pub fn get_foreground_window_info() -> (Option<String>, Option<String>) {
    (None, None)
}

#[cfg(not(target_os = "windows"))]
pub fn get_idle_duration_ms() -> Option<u64> {
    None
}

#[cfg(not(target_os = "windows"))]
pub fn get_idle_state(_afk_timeout_secs: u64) -> (crate::domain::IdleState, Option<u64>) {
    (crate::domain::IdleState::Unknown, None)
}

/// Returns the local machine hostname.
pub fn get_local_hostname() -> Option<String> {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
}

/// Returns OS and Win32 environment description.
pub fn get_os_environment_info() -> String {
    #[cfg(target_os = "windows")]
    {
        "Windows Win32 Subsystem (User32 / Kernel32)".to_string()
    }
    #[cfg(not(target_os = "windows"))]
    {
        "Non-Windows Environment (Stub)".to_string()
    }
}
