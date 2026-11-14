//! Per-application system audio on macOS 14.2+: a Core Audio process tap on just the
//! meeting app's processes, wrapped in a private aggregate device that cpal can record
//! like any input. Mirrors cpal's own system-wide tap (`cpal::host::coreaudio::macos::
//! loopback`), but lists the processes to include instead of excluding none.
//!
//! Meeting apps often play audio from a helper process (Chrome's audio service, Teams'
//! WebView), so every audio process whose parent chain leads back to the picked app is
//! included. Safari plays web audio from a system process (`com.apple.WebKit.GPU`) that
//! is not its child, so a Safari call finds nothing here and the caller falls back to
//! recording everything playing.

use std::ffi::{c_void, CStr};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, Ordering};

use objc2::AnyThread;
use objc2_core_audio::{
    kAudioAggregateDeviceIsPrivateKey, kAudioAggregateDeviceNameKey, kAudioAggregateDeviceTapAutoStartKey,
    kAudioAggregateDeviceTapListKey, kAudioAggregateDeviceUIDKey, kAudioHardwarePropertyProcessObjectList,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, kAudioProcessPropertyPID,
    kAudioSubTapDriftCompensationKey, kAudioSubTapUIDKey, AudioHardwareCreateAggregateDevice, AudioHardwareCreateProcessTap,
    AudioHardwareDestroyAggregateDevice, AudioHardwareDestroyProcessTap, AudioObjectGetPropertyData,
    AudioObjectGetPropertyDataSize, AudioObjectID, AudioObjectPropertyAddress, CATapDescription, CATapMuteBehavior,
};
use objc2_core_foundation::{
    kCFAllocatorDefault, kCFTypeArrayCallBacks, kCFTypeDictionaryKeyCallBacks, kCFTypeDictionaryValueCallBacks, CFArray,
    CFDictionary, CFMutableDictionary, CFRetained, CFString,
};
use objc2_foundation::{NSArray, NSNumber, NSString};

static INSTANCE: AtomicU32 = AtomicU32::new(0);

/// A process tap plus the aggregate device that exposes it; both are destroyed on drop.
pub struct AppTap {
    tap_id: AudioObjectID,
    aggregate_id: AudioObjectID,
    /// UID of the aggregate device, to find it among cpal's devices.
    pub uid: String,
    /// How many audio processes the tap covers.
    pub processes: usize,
}

impl Drop for AppTap {
    fn drop(&mut self) {
        // SAFETY: both ids were created by this struct and are destroyed exactly once.
        unsafe {
            let _ = AudioHardwareDestroyAggregateDevice(self.aggregate_id);
            let _ = AudioHardwareDestroyProcessTap(self.tap_id);
        }
    }
}

/// Tap the audio of `pid` and its child processes.
pub fn create(pid: u32) -> Result<AppTap, String> {
    let objects: Vec<AudioObjectID> = audio_processes().into_iter().filter(|&(_, p)| descends_from(p, pid)).map(|(o, _)| o).collect();
    if objects.is_empty() {
        return Err("that app is not playing audio through its own process".into());
    }
    let numbers: Vec<objc2::rc::Retained<NSNumber>> = objects.iter().map(|&o| NSNumber::new_u32(o)).collect();
    let list = NSArray::from_retained_slice(&numbers);
    let instance = INSTANCE.fetch_add(1, Ordering::Relaxed);
    let me = std::process::id();

    // SAFETY: plain Objective-C object construction and setters on a fresh description.
    let desc = unsafe { CATapDescription::initStereoMixdownOfProcesses(CATapDescription::alloc(), &list) };
    unsafe {
        desc.setMuteBehavior(CATapMuteBehavior::Unmuted); // the user still hears the call
        desc.setName(&NSString::from_str(&format!("Debrief app audio {me}.{instance}")));
        desc.setPrivate(true);
    }
    let mut tap_id: AudioObjectID = 0;
    // SAFETY: `desc` is a valid description; `tap_id` is a valid out-pointer.
    check(unsafe { AudioHardwareCreateProcessTap(Some(&*desc), &mut tap_id) }, "create process tap")?;
    let tap_uid = unsafe { desc.UUID().UUIDString() };

    let uid = format!("app.debrief.apptap.{me}.{instance}");
    let props = aggregate_properties(&tap_uid, &uid, &format!("Debrief app audio {me}.{instance}"));
    let mut aggregate_id: AudioObjectID = 0;
    // SAFETY: `props` is a well-formed aggregate description; out-pointer is valid.
    let status = unsafe { AudioHardwareCreateAggregateDevice(&props, NonNull::from(&mut aggregate_id)) };
    if let Err(e) = check(status, "create aggregate device") {
        unsafe {
            let _ = AudioHardwareDestroyProcessTap(tap_id);
        }
        return Err(e);
    }
    Ok(AppTap { tap_id, aggregate_id, uid, processes: objects.len() })
}

fn check(status: i32, what: &str) -> Result<(), String> {
    if status == 0 {
        Ok(())
    } else {
        Err(format!("{what} failed (OSStatus {status})"))
    }
}

fn address(selector: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress { mSelector: selector, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain }
}

/// Every process Core Audio knows about, as (process object, pid).
fn audio_processes() -> Vec<(AudioObjectID, u32)> {
    let system = kAudioObjectSystemObject as AudioObjectID;
    let addr = address(kAudioHardwarePropertyProcessObjectList);
    let mut size: u32 = 0;
    // SAFETY: querying a property size on the system object.
    if unsafe { AudioObjectGetPropertyDataSize(system, NonNull::from(&addr), 0, std::ptr::null(), NonNull::from(&mut size)) } != 0 {
        return Vec::new();
    }
    let mut ids = vec![0 as AudioObjectID; size as usize / std::mem::size_of::<AudioObjectID>()];
    if ids.is_empty() {
        return Vec::new();
    }
    // SAFETY: `ids` holds `size` bytes, as Core Audio just reported.
    let status = unsafe {
        AudioObjectGetPropertyData(system, NonNull::from(&addr), 0, std::ptr::null(), NonNull::from(&mut size), NonNull::new_unchecked(ids.as_mut_ptr().cast::<c_void>()))
    };
    if status != 0 {
        return Vec::new();
    }
    ids.truncate(size as usize / std::mem::size_of::<AudioObjectID>());
    ids.into_iter()
        .filter_map(|obj| {
            let addr = address(kAudioProcessPropertyPID);
            let mut pid: i32 = 0;
            let mut size = std::mem::size_of::<i32>() as u32;
            // SAFETY: kAudioProcessPropertyPID is a pid_t.
            let ok = unsafe {
                AudioObjectGetPropertyData(obj, NonNull::from(&addr), 0, std::ptr::null(), NonNull::from(&mut size), NonNull::from(&mut pid).cast())
            } == 0;
            (ok && pid > 0).then_some((obj, pid as u32))
        })
        .collect()
}

/// True when `pid` is `root` or one of its descendants.
fn descends_from(mut pid: u32, root: u32) -> bool {
    for _ in 0..32 {
        if pid == root {
            return true;
        }
        match parent_of(pid) {
            Some(p) if p > 1 && p != pid => pid = p,
            _ => return false,
        }
    }
    false
}

fn parent_of(pid: u32) -> Option<u32> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    // SAFETY: `info` is a zeroed proc_bsdinfo of exactly `size` bytes.
    let n = unsafe { libc::proc_pidinfo(pid as i32, libc::PROC_PIDTBSDINFO, 0, (&mut info as *mut libc::proc_bsdinfo).cast(), size) };
    (n == size).then_some(info.pbi_ppid)
}

/// `dict[key] = value`; the dictionary retains both.
///
/// # Safety
/// `value` must point to a live CoreFoundation (or toll-free bridged) object.
unsafe fn set(dict: &CFMutableDictionary, key: &'static CStr, value: *const c_void) {
    let k = cf_key(key);
    CFMutableDictionary::set_value(Some(dict), &*k as *const CFString as *const c_void, value);
}

fn cf_key(key: &'static CStr) -> CFRetained<CFString> {
    // SAFETY: the Core Audio key constants are NUL-terminated UTF-8.
    unsafe { CFString::with_c_string(kCFAllocatorDefault, key.as_ptr(), 0x0800_0100 /* UTF-8 */) }.expect("CFString from a constant")
}

/// The aggregate-device description cpal uses for its tap, private to this process.
fn aggregate_properties(tap_uid: &NSString, uid: &str, name: &str) -> CFRetained<CFDictionary> {
    let yes = NSNumber::new_bool(true);
    // SAFETY: CoreFoundation containers built with the standard CFType callbacks; every
    // key and value is a live CF/NS object (NSString and NSNumber are toll-free bridged).
    unsafe {
        let sub_tap = CFMutableDictionary::new(kCFAllocatorDefault, 2, &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks).expect("CFMutableDictionary");
        set(&sub_tap, kAudioSubTapUIDKey, tap_uid as *const NSString as *const c_void);
        set(&sub_tap, kAudioSubTapDriftCompensationKey, &*yes as *const NSNumber as *const c_void);
        let taps_list = [sub_tap];
        let taps = CFArray::new(kCFAllocatorDefault, taps_list.as_ptr() as *mut *const c_void, taps_list.len() as _, &kCFTypeArrayCallBacks).expect("CFArray");

        let dict = CFMutableDictionary::new(kCFAllocatorDefault, 5, &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks).expect("CFMutableDictionary");
        let (name, uid) = (CFString::from_str(name), CFString::from_str(uid));
        set(&dict, kAudioAggregateDeviceNameKey, &*name as *const CFString as *const c_void);
        set(&dict, kAudioAggregateDeviceUIDKey, &*uid as *const CFString as *const c_void);
        set(&dict, kAudioAggregateDeviceTapListKey, &*taps as *const CFArray as *const c_void);
        set(&dict, kAudioAggregateDeviceTapAutoStartKey, &*yes as *const NSNumber as *const c_void);
        set(&dict, kAudioAggregateDeviceIsPrivateKey, &*yes as *const NSNumber as *const c_void);
        CFRetained::cast_unchecked::<CFDictionary>(dict)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_descends_from_itself_and_its_parent() {
        let me = std::process::id();
        assert!(descends_from(me, me));
        let parent = parent_of(me).expect("own parent pid");
        assert!(descends_from(me, parent));
        assert!(!descends_from(parent, me));
    }

    #[test]
    fn enumerating_audio_processes_does_not_fail() {
        // CI machines may have no audio processes at all; this only has to run cleanly.
        let _ = audio_processes();
    }
}
