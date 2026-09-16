//! Per-application system audio on Windows: WASAPI "process loopback"
//! (Windows 10 2004+) captures only what one process tree plays, so music or
//! notifications from other apps never end up in a meeting recording.

use crate::dsp::Resampler;
use crate::stream::{AudioSink, ErrorSink, StreamError};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;
use windows::core::{implement, Interface, Ref, Result as WinResult};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Media::Audio::{
    ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler,
    IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient, AUDCLNT_BUFFERFLAGS_SILENT,
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK, AUDIOCLIENT_ACTIVATION_PARAMS,
    AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
    PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVEFORMATEX,
};
use windows::Win32::Media::Multimedia::WAVE_FORMAT_IEEE_FLOAT;
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, BLOB, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject};
use windows::Win32::System::Variant::VT_BLOB;

/// Format we ask the loopback client for (process loopback has no mix format
/// to query, the caller picks one).
const CAPTURE_HZ: u32 = 48_000;
const CHANNELS: u16 = 2;

/// A running per-process capture. Dropping it stops the thread without
/// waiting for it (a wedged WASAPI call must never block the app).
pub struct ProcessLoopback {
    stop: Arc<AtomicBool>,
    wake: HANDLE,
    _thread: Option<JoinHandle<()>>,
}

unsafe impl Send for ProcessLoopback {}

impl Drop for ProcessLoopback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        unsafe {
            let _ = SetEvent(self.wake);
        }
        // fable: the event handle is leaked on purpose (one per recording) - closing it
        // here could race the capture thread's WaitForSingleObject, and joining could hang.
    }
}

#[implement(IActivateAudioInterfaceCompletionHandler)]
struct Completion {
    tx: crossbeam_channel::Sender<WinResult<IAudioClient>>,
}

impl IActivateAudioInterfaceCompletionHandler_Impl for Completion_Impl {
    fn ActivateCompleted(&self, op: Ref<IActivateAudioInterfaceAsyncOperation>) -> WinResult<()> {
        let result = (|| -> WinResult<IAudioClient> {
            let op = op.ok()?;
            let mut hr = windows::core::HRESULT(0);
            let mut unk = None;
            unsafe { op.GetActivateResult(&mut hr, &mut unk)? };
            hr.ok()?;
            let unk: windows::core::IUnknown = unk.ok_or_else(|| windows::core::Error::from(hr))?;
            unk.cast::<IAudioClient>()
        })();
        let _ = self.tx.send(result);
        Ok(())
    }
}

/// Start capturing the audio of `pid` (and its child processes). `on_audio`
/// receives mono f32 at `target_hz`.
pub fn open_process_loopback(pid: u32, target_hz: u32, on_audio: AudioSink, on_error: ErrorSink) -> Result<ProcessLoopback, StreamError> {
    let stop = Arc::new(AtomicBool::new(false));
    let wake = unsafe { CreateEventW(None, false, false, None) }.map_err(|e| StreamError::Cpal(e.to_string()))?;
    let (ready_tx, ready_rx) = crossbeam_channel::bounded::<Result<(), String>>(1);
    let stop2 = stop.clone();
    let wake_raw = wake.0 as isize;
    let thread = std::thread::Builder::new()
        .name("hark-process-loopback".into())
        .spawn(move || {
            let wake = HANDLE(wake_raw as *mut _);
            let r = unsafe { run(pid, target_hz, wake, stop2, on_audio, on_error, ready_tx) };
            if let Err(e) = r {
                tracing::warn!("process loopback ended: {e}");
            }
        })
        .map_err(|e| StreamError::Cpal(e.to_string()))?;
    match ready_rx.recv_timeout(Duration::from_secs(8)) {
        Ok(Ok(())) => Ok(ProcessLoopback { stop, wake, _thread: Some(thread) }),
        Ok(Err(e)) => {
            stop.store(true, Ordering::SeqCst);
            Err(StreamError::Cpal(e))
        }
        Err(_) => {
            stop.store(true, Ordering::SeqCst);
            Err(StreamError::Cpal("process loopback did not start in time".into()))
        }
    }
}

unsafe fn run(
    pid: u32,
    target_hz: u32,
    wake: HANDLE,
    stop: Arc<AtomicBool>,
    on_audio: AudioSink,
    on_error: ErrorSink,
    ready: crossbeam_channel::Sender<Result<(), String>>,
) -> Result<(), String> {
    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    let result = (|| -> Result<(), String> {
        // Activation parameters travel as a VT_BLOB PROPVARIANT.
        let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS { TargetProcessId: pid, ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE },
            },
        };
        // ManuallyDrop: PROPVARIANT's Drop would PropVariantClear a blob that points at our stack.
        let blob = std::mem::ManuallyDrop::new(PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_BLOB,
                    wReserved1: 0,
                    wReserved2: 0,
                    wReserved3: 0,
                    Anonymous: PROPVARIANT_0_0_0 {
                        blob: BLOB { cbSize: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32, pBlobData: &mut params as *mut _ as *mut u8 },
                    },
                }),
            },
        });
        let (tx, rx) = crossbeam_channel::bounded::<WinResult<IAudioClient>>(1);
        let handler: IActivateAudioInterfaceCompletionHandler = Completion { tx }.into();
        let _op = ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, &IAudioClient::IID, Some(&*blob as *const PROPVARIANT), &handler)
            .map_err(|e| format!("activate: {e}"))?;
        let client = rx.recv_timeout(Duration::from_secs(5)).map_err(|_| "activation timed out".to_string())?.map_err(|e| format!("activate: {e}"))?;

        let fmt = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_IEEE_FLOAT as u16,
            nChannels: CHANNELS,
            nSamplesPerSec: CAPTURE_HZ,
            nAvgBytesPerSec: CAPTURE_HZ * CHANNELS as u32 * 4,
            nBlockAlign: CHANNELS * 4,
            wBitsPerSample: 32,
            cbSize: 0,
        };
        client
            .Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK, 2_000_000, 0, &fmt, None)
            .map_err(|e| format!("initialize: {e}"))?;
        client.SetEventHandle(wake).map_err(|e| format!("event: {e}"))?;
        let capture: IAudioCaptureClient = client.GetService().map_err(|e| format!("capture client: {e}"))?;
        client.Start().map_err(|e| format!("start: {e}"))?;
        let _ = ready.send(Ok(()));

        let mut resampler = Resampler::new(CAPTURE_HZ, target_hz);
        let mut mono: Vec<f32> = Vec::with_capacity(4800);
        loop {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            // Process loopback does not always signal the event; poll on timeout too.
            let _ = WaitForSingleObject(wake, 100);
            if stop.load(Ordering::SeqCst) {
                break;
            }
            loop {
                let packet = match capture.GetNextPacketSize() {
                    Ok(n) => n,
                    Err(e) => {
                        on_error(format!("process loopback: {e}"));
                        return Err(e.to_string());
                    }
                };
                if packet == 0 {
                    break;
                }
                let mut data: *mut u8 = std::ptr::null_mut();
                let mut frames = 0u32;
                let mut flags = 0u32;
                if let Err(e) = capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None) {
                    on_error(format!("process loopback: {e}"));
                    return Err(e.to_string());
                }
                mono.clear();
                if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                    mono.resize(frames as usize, 0.0);
                } else {
                    let samples = std::slice::from_raw_parts(data as *const f32, frames as usize * CHANNELS as usize);
                    mono.extend(samples.chunks(CHANNELS as usize).map(|f| f.iter().sum::<f32>() / CHANNELS as f32));
                }
                let _ = capture.ReleaseBuffer(frames);
                let out = resampler.process(&mono);
                if !out.is_empty() {
                    on_audio(&out);
                }
            }
        }
        let _ = client.Stop();
        Ok(())
    })();
    if let Err(e) = &result {
        let _ = ready.send(Err(e.clone()));
    }
    CoUninitialize();
    result
}
