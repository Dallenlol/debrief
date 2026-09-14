//! Enumerate visible top-level windows with their owning process name.

use crate::WindowInfo;

/// Visible windows with a non-empty title. Never panics; returns an empty
/// list on platforms/errors where enumeration is unavailable.
pub fn list_visible_windows() -> Vec<WindowInfo> {
    imp::list()
}

#[cfg(windows)]
mod imp {
    use super::WindowInfo;
    use windows::core::{BOOL, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    };

    pub fn list() -> Vec<WindowInfo> {
        let mut out: Vec<WindowInfo> = Vec::new();
        // SAFETY: the callback only touches `out` through the LPARAM pointer for the
        // duration of EnumWindows, which is synchronous.
        unsafe {
            let _ = EnumWindows(Some(cb), LPARAM(&mut out as *mut Vec<WindowInfo> as isize));
        }
        out
    }

    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let out = &mut *(lparam.0 as *mut Vec<WindowInfo>);
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return BOOL(1);
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, &mut buf);
        if n <= 0 {
            return BOOL(1);
        }
        let title = String::from_utf16_lossy(&buf[..n as usize]);
        if title.trim().is_empty() {
            return BOOL(1);
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let process = process_name(pid).unwrap_or_default();
        out.push(WindowInfo { process, title });
        BOOL(1)
    }

    fn process_name(pid: u32) -> Option<String> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = vec![0u16; 1024];
            let mut size = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut size).is_ok();
            let _ = CloseHandle(h);
            if !ok {
                return None;
            }
            let full = String::from_utf16_lossy(&buf[..size as usize]);
            let base = full.rsplit(['\\', '/']).next().unwrap_or(&full);
            Some(base.to_lowercase())
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::WindowInfo;
    use core_foundation::array::CFArray;
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use core_graphics::window::{
        kCGNullWindowID, kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly,
        CGWindowListCopyWindowInfo,
    };

    pub fn list() -> Vec<WindowInfo> {
        let mut out = Vec::new();
        let opts = kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements;
        let raw = unsafe { CGWindowListCopyWindowInfo(opts, kCGNullWindowID) };
        if raw.is_null() {
            return out;
        }
        let arr: CFArray<CFDictionary<CFString, CFType>> = unsafe { CFArray::wrap_under_create_rule(raw) };
        for dict in arr.iter() {
            let layer = get_i64(&dict, "kCGWindowLayer").unwrap_or(0);
            if layer != 0 {
                continue;
            }
            let title = get_str(&dict, "kCGWindowName").unwrap_or_default();
            if title.trim().is_empty() {
                continue;
            }
            let owner = get_str(&dict, "kCGWindowOwnerName").unwrap_or_default().to_lowercase();
            out.push(WindowInfo { process: owner, title });
        }
        out
    }

    fn get_str(d: &CFDictionary<CFString, CFType>, key: &str) -> Option<String> {
        let v = d.find(CFString::new(key))?;
        v.downcast::<CFString>().map(|s| s.to_string())
    }

    fn get_i64(d: &CFDictionary<CFString, CFType>, key: &str) -> Option<i64> {
        let v = d.find(CFString::new(key))?;
        v.downcast::<CFNumber>().and_then(|n| n.to_i64())
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use super::WindowInfo;
    pub fn list() -> Vec<WindowInfo> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumeration_does_not_panic() {
        let _ = list_visible_windows();
    }

    #[test]
    #[ignore = "needs an interactive desktop"]
    fn lists_something() {
        let w = list_visible_windows();
        assert!(!w.is_empty());
        assert!(w.iter().all(|x| !x.title.trim().is_empty()));
    }
}
