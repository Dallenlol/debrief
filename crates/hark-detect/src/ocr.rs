//! People's names read off a screenshot of the call (Windows' built-in OCR, or Apple's
//! Vision text recognition on macOS), for the
//! name labels on video tiles that a meeting app does not expose to accessibility.
//! Names are only ever *suggested*, and only kept once seen in more than one frame.

use crate::participants::extract_names;
use std::path::Path;

/// Names in the image at `path`. Empty when OCR is unavailable (Linux, no OCR language).
pub fn names_in_image(path: &Path) -> Vec<String> {
    names_from_lines(&ocr_lines(path))
}

/// The name-shaped lines among OCR output (two to four capitalised words, no UI vocabulary).
pub fn names_from_lines(lines: &[String]) -> Vec<String> {
    extract_names("ocr", lines)
}

#[cfg(windows)]
fn ocr_lines(path: &Path) -> Vec<String> {
    match imp::lines(path) {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("ocr {}: {e}", path.display());
            Vec::new()
        }
    }
}

#[cfg(target_os = "macos")]
fn ocr_lines(path: &Path) -> Vec<String> {
    mac::lines(path)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn ocr_lines(_path: &Path) -> Vec<String> {
    Vec::new()
}

/// Apple Vision: one recognised line per text observation, best candidate only.
#[cfg(target_os = "macos")]
mod mac {
    use objc2::AnyThread;
    use objc2_foundation::{NSArray, NSDictionary, NSString, NSURL};
    use objc2_vision::{VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel};
    use std::path::Path;

    pub fn lines(path: &Path) -> Vec<String> {
        let Some(p) = path.to_str() else { return Vec::new() };
        #[allow(unused_unsafe)]
        // SAFETY: Vision objects built from a file URL and used on this thread only.
        unsafe {
            let url = NSURL::fileURLWithPath(&NSString::from_str(p));
            let handler = VNImageRequestHandler::initWithURL_options(VNImageRequestHandler::alloc(), &url, &NSDictionary::new());
            let request = VNRecognizeTextRequest::new();
            request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
            request.setUsesLanguageCorrection(false); // names are not dictionary words
            let as_request: &VNRequest = &request;
            if let Err(e) = handler.performRequests_error(&NSArray::from_slice(&[as_request])) {
                tracing::warn!("ocr {}: {}", path.display(), e.localizedDescription());
                return Vec::new();
            }
            request
                .results()
                .map(|obs| obs.iter().filter_map(|o| o.topCandidates(1).firstObject().map(|t| t.string().to_string())).collect())
                .unwrap_or_default()
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::path::Path;
    use windows::core::HSTRING;
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::{FileAccessMode, StorageFile};
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    pub fn lines(path: &Path) -> windows::core::Result<Vec<String>> {
        // WinRT needs the thread in an apartment; harmless if it already is.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let abs = std::fs::canonicalize(path).map_err(|e| windows::core::Error::new(windows::core::HRESULT(-1), e.to_string()))?;
        let abs = abs.to_string_lossy().trim_start_matches(r"\\?\").to_string();
        let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(abs))?.join()?;
        let stream = file.OpenAsync(FileAccessMode::Read)?.join()?;
        let bitmap = BitmapDecoder::CreateAsync(&stream)?.join()?.GetSoftwareBitmapAsync()?.join()?;
        let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
        let result = engine.RecognizeAsync(&bitmap)?.join()?;
        let mut out = Vec::new();
        for line in result.Lines()? {
            out.push(line.Text()?.to_string_lossy());
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_tile_labels_and_drops_screen_text() {
        let lines: Vec<String> = ["Priya Sharma", "Jordan Lee (Host)", "Mute", "Share Screen", "Q3 Roadmap Review 2026", "ok", "Marcus Webb is presenting", "https://x.com"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(names_from_lines(&lines), vec!["Priya Sharma", "Jordan Lee", "Marcus Webb"]);
    }

    /// Reads real text with Windows OCR. Run: HARK_TEST_OCR=path\to\image.png cargo test -p hark-detect ocr -- --ignored
    #[test]
    #[ignore]
    fn reads_an_image() {
        let p = std::env::var("HARK_TEST_OCR").expect("set HARK_TEST_OCR");
        let lines = ocr_lines(Path::new(&p));
        println!("{lines:#?}");
        assert!(!lines.is_empty());
    }
}
