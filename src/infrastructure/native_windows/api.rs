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

    pub type LPARAM = isize;
    pub type WNDENUMPROC = Option<unsafe extern "system" fn(HWND, LPARAM) -> BOOL>;

    #[link(name = "user32")]
    extern "system" {
        pub fn GetForegroundWindow() -> HWND;
        pub fn GetWindowTextW(hWnd: HWND, lpString: *mut WCHAR, nMaxCount: i32) -> i32;
        pub fn GetWindowThreadProcessId(hWnd: HWND, lpdwProcessId: *mut DWORD) -> DWORD;
        pub fn GetLastInputInfo(plii: *mut LASTINPUTINFO) -> BOOL;
        pub fn EnumWindows(lpEnumFunc: WNDENUMPROC, lParam: LPARAM) -> BOOL;
        pub fn IsWindowVisible(hWnd: HWND) -> BOOL;
        pub fn MessageBeep(uType: UINT) -> BOOL;
        pub fn FlashWindow(hWnd: HWND, bInvert: BOOL) -> BOOL;
        pub fn SetForegroundWindow(hWnd: HWND) -> BOOL;
        pub fn ShowWindow(hWnd: HWND, nCmdShow: i32) -> BOOL;
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

/// Metadata describing a running desktop application for novice-friendly workflow creation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunningAppInfo {
    pub process_name: String,
    pub display_name: String,
    pub window_title: String,
}

/// Enumerates all currently visible top-level application windows on the user's desktop.
#[cfg(target_os = "windows")]
pub fn list_running_applications() -> Vec<RunningAppInfo> {
    let mut apps: Vec<RunningAppInfo> = Vec::new();
    let lparam = &mut apps as *mut Vec<RunningAppInfo> as win32::LPARAM;

    unsafe {
        win32::EnumWindows(Some(enum_windows_callback), lparam);
    }

    // Deduplicate by process_name
    let mut seen = std::collections::HashSet::new();
    apps.retain(|a| seen.insert(a.process_name.clone()));

    // Sort alphabetically by display name
    apps.sort_by_key(|a| a.display_name.to_lowercase());
    apps
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn enum_windows_callback(
    hwnd: win32::HWND,
    lparam: win32::LPARAM,
) -> win32::BOOL {
    if win32::IsWindowVisible(hwnd) == 0 {
        return 1;
    }

    let mut title_buf = [0u16; 512];
    let title_len = win32::GetWindowTextW(hwnd, title_buf.as_mut_ptr(), title_buf.len() as i32);
    if title_len <= 0 {
        return 1;
    }

    let raw_title = String::from_utf16_lossy(&title_buf[..title_len as usize]);
    let title = raw_title.trim();
    if title.is_empty() {
        return 1;
    }

    let lower_title = title.to_lowercase();
    let ignored_titles = [
        "default ime",
        "msctfime ui",
        "program manager",
        "settings",
        "windows input experience",
        "task switching",
        "snap assist",
    ];
    if ignored_titles
        .iter()
        .any(|&ig| lower_title == ig || lower_title.starts_with(ig))
    {
        return 1;
    }

    let mut pid: win32::DWORD = 0;
    win32::GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == 0 {
        return 1;
    }

    let handle = win32::OpenProcess(win32::PROCESS_QUERY_LIMITED_INFORMATION, win32::FALSE, pid);
    if handle.is_null() {
        return 1;
    }

    let mut path_buf = [0u16; 1024];
    let mut size: win32::DWORD = path_buf.len() as win32::DWORD;
    let success = win32::QueryFullProcessImageNameW(handle, 0, path_buf.as_mut_ptr(), &mut size);
    win32::CloseHandle(handle);

    if success != 0 && size > 0 {
        let full_path = String::from_utf16_lossy(&path_buf[..size as usize]);
        if let Some(app_name) =
            crate::infrastructure::activitywatch::mapper::sanitize_app_name(&full_path)
        {
            let lower_app = app_name.to_lowercase();
            if lower_app == "searchhost.exe"
                || lower_app == "shellexperiencehost.exe"
                || lower_app == "startmenuexperiencehost.exe"
            {
                return 1;
            }

            let display = friendly_display_name(&app_name, title);
            let apps = &mut *(lparam as *mut Vec<RunningAppInfo>);
            apps.push(RunningAppInfo {
                process_name: app_name,
                display_name: display,
                window_title: title.to_string(),
            });
        }
    }

    1
}

pub fn friendly_display_name(app_name: &str, window_title: &str) -> String {
    let lower = app_name.to_lowercase();
    let title_lower = window_title.to_lowercase();

    if title_lower.contains("antigravity") || lower.contains("antigravity") {
        return "Antigravity".to_string();
    }

    match lower.as_str() {
        "code.exe" => "Visual Studio Code".to_string(),
        "chrome.exe" => "Google Chrome".to_string(),
        "msedge.exe" => "Microsoft Edge".to_string(),
        "brave.exe" => "Brave Browser".to_string(),
        "firefox.exe" => "Mozilla Firefox".to_string(),
        "windowsterminal.exe" => "Windows Terminal".to_string(),
        "cmd.exe" => "Command Prompt".to_string(),
        "powershell.exe" | "pwsh.exe" => "PowerShell".to_string(),
        "notepad.exe" => "Notepad".to_string(),
        "obsidian.exe" => "Obsidian".to_string(),
        "notion.exe" => "Notion".to_string(),
        "slack.exe" => "Slack".to_string(),
        "discord.exe" => "Discord".to_string(),
        "spotify.exe" => "Spotify".to_string(),
        "steam.exe" => "Steam".to_string(),
        "devenv.exe" => "Visual Studio".to_string(),
        "idea64.exe" | "idea.exe" => "IntelliJ IDEA".to_string(),
        "pycharm64.exe" | "pycharm.exe" => "PyCharm".to_string(),
        "winword.exe" => "Microsoft Word".to_string(),
        "excel.exe" => "Microsoft Excel".to_string(),
        "powerpnt.exe" => "Microsoft PowerPoint".to_string(),
        "onenote.exe" => "Microsoft OneNote".to_string(),
        "acrobat.exe" => "Adobe Acrobat".to_string(),
        "foxitreader.exe" => "Foxit PDF Reader".to_string(),
        _ => {
            let base = lower.trim_end_matches(".exe");
            let mut chars = base.chars();
            match chars.next() {
                None => app_name.to_string(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        }
    }
}

#[cfg(target_os = "windows")]
pub fn is_window_subsystem_available() -> bool {
    unsafe {
        let _ = win32::GetForegroundWindow();
        true
    }
}

// ----------------------------------------------------------------------------
// Non-Windows fallbacks (for compiling on Linux/macOS in CI/mock environments)
// ----------------------------------------------------------------------------

#[cfg(not(target_os = "windows"))]
pub fn is_window_subsystem_available() -> bool {
    false
}

#[cfg(not(target_os = "windows"))]
pub fn list_running_applications() -> Vec<RunningAppInfo> {
    vec![]
}

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

/// Plays a native Windows system warning sound to physically alert the user of focus violations.
#[cfg(target_os = "windows")]
pub fn play_system_alert() {
    unsafe {
        // 0x30 = MB_ICONEXCLAMATION / MB_ICONWARNING
        let _ = win32::MessageBeep(0x0000_0030);
    }
}

#[cfg(not(target_os = "windows"))]
pub fn play_system_alert() {}

/// Plays an urgent warning sound and brings the FlowGuard desktop window to the front
/// (or flashes its taskbar button in amber) to actively confront the user when grace expires.
#[cfg(target_os = "windows")]
pub fn alert_focus_violation() {
    unsafe {
        // 1. Play native Windows exclamation/warning beep
        let _ = win32::MessageBeep(0x0000_0030);

        // 2. Locate the FlowGuard application window and bring to attention
        win32::EnumWindows(Some(find_and_flash_flowguard_window), 0);
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn find_and_flash_flowguard_window(
    hwnd: win32::HWND,
    _lparam: win32::LPARAM,
) -> win32::BOOL {
    if win32::IsWindowVisible(hwnd) == 0 {
        return 1;
    }
    let mut title_buf = [0u16; 512];
    let title_len = win32::GetWindowTextW(hwnd, title_buf.as_mut_ptr(), title_buf.len() as i32);
    if title_len > 0 {
        let title = String::from_utf16_lossy(&title_buf[..title_len as usize]);
        if title.contains("FlowGuard") {
            win32::ShowWindow(hwnd, 9); // SW_RESTORE
            win32::SetForegroundWindow(hwnd);
            win32::FlashWindow(hwnd, 1);
            return 0; // Stop enumeration once found
        }
    }
    1
}

#[cfg(not(target_os = "windows"))]
pub fn alert_focus_violation() {}

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
