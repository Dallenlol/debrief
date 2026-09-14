//! `.hark` bundles: a zip with `manifest.json` plus each meeting's media files.
//! Import merges by meeting id, so re-importing is safe.

use crate::{Result, ShareError};
use hark_store::{Meeting, MeetingSpeaker, NewSegment, Segment, Store, Summary};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use zip::write::SimpleFileOptions;

pub const BUNDLE_VERSION: u32 = 1;
const MEDIA_FILES: &[&str] = &["mix.wav", "mic.wav", "sys.wav", "screen.mp4"];

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BundleMeeting {
    pub meeting: Meeting,
    pub segments: Vec<Segment>,
    pub speakers: Vec<MeetingSpeaker>,
    pub summary: Option<Summary>,
    pub highlights: Vec<u64>,
    pub tags: Vec<String>,
    /// File names inside `media/<meeting-id>/`.
    pub media: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BundleManifest {
    pub version: u32,
    pub exported_at: chrono::DateTime<chrono::Utc>,
    pub app: String,
    pub meetings: Vec<BundleMeeting>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct ImportReport {
    pub imported: usize,
    pub skipped_existing: usize,
    pub errors: Vec<String>,
}

fn collect(store: &Store, meeting_id: &str) -> Result<BundleMeeting> {
    let meeting = store.get_meeting(meeting_id)?.ok_or_else(|| ShareError::Other(format!("meeting {meeting_id} not found")))?;
    Ok(BundleMeeting {
        segments: store.segments(meeting_id)?,
        speakers: store.meeting_speakers(meeting_id)?,
        summary: store.get_summary(meeting_id)?,
        highlights: store.get_setting(&format!("highlights:{meeting_id}"))?.unwrap_or_default(),
        tags: store.meeting_tags(meeting_id)?.into_iter().map(|t| t.name).collect(),
        media: MEDIA_FILES.iter().filter(|f| store.recordings_dir(meeting_id).join(f).exists()).map(|f| f.to_string()).collect(),
        meeting,
    })
}

/// Write a bundle with the given meetings. `include_video` false drops `screen.mp4`.
pub fn export_bundle(store: &Store, meeting_ids: &[String], out: &Path, include_video: bool) -> Result<BundleManifest> {
    let mut meetings = Vec::new();
    for id in meeting_ids {
        let mut m = collect(store, id)?;
        if !include_video {
            m.media.retain(|f| f != "screen.mp4");
        }
        meetings.push(m);
    }
    let manifest = BundleManifest { version: BUNDLE_VERSION, exported_at: chrono::Utc::now(), app: "hark".into(), meetings };
    let file = File::create(out)?;
    let mut zip = zip::ZipWriter::new(file);
    let deflate = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("manifest.json", deflate)?;
    zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
    for m in &manifest.meetings {
        let dir = store.recordings_dir(&m.meeting.id);
        for f in &m.media {
            let p = dir.join(f);
            // Media is already compressed (or huge); store without deflate to keep export fast.
            zip.start_file(format!("media/{}/{}", m.meeting.id, f), stored)?;
            let mut src = File::open(&p)?;
            std::io::copy(&mut src, &mut zip)?;
        }
    }
    zip.finish()?;
    Ok(manifest)
}

/// Read only the manifest (for a preview before importing).
pub fn read_manifest(bundle: &Path) -> Result<BundleManifest> {
    let mut zip = zip::ZipArchive::new(File::open(bundle)?)?;
    let mut f = zip.by_name("manifest.json")?;
    let mut s = String::new();
    f.read_to_string(&mut s)?;
    Ok(serde_json::from_str(&s)?)
}

/// Import a bundle, skipping meetings that already exist.
pub fn import_bundle(store: &Store, bundle: &Path) -> Result<ImportReport> {
    let manifest = read_manifest(bundle)?;
    let mut zip = zip::ZipArchive::new(File::open(bundle)?)?;
    let mut report = ImportReport::default();
    for bm in &manifest.meetings {
        let id = &bm.meeting.id;
        if store.get_meeting(id)?.is_some() {
            report.skipped_existing += 1;
            continue;
        }
        let res: Result<()> = (|| {
            let dir = store.recordings_dir(id);
            for f in &bm.media {
                let name = format!("media/{id}/{f}");
                let mut entry = match zip.by_name(&name) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                let mut out = File::create(dir.join(f))?;
                std::io::copy(&mut entry, &mut out)?;
            }
            let mut meeting = bm.meeting.clone();
            meeting.folder_id = None; // folders are local
            meeting.has_video = bm.media.iter().any(|f| f == "screen.mp4");
            store.create_meeting(&meeting)?;
            let segs: Vec<NewSegment> = bm.segments.iter().map(|s| NewSegment { start_ms: s.start_ms, end_ms: s.end_ms, speaker: s.speaker.clone(), text: s.text.clone() }).collect();
            store.replace_segments(id, &segs)?;
            let stored = store.segments(id)?;
            let clean: Vec<(i64, String)> = stored.iter().zip(&bm.segments).filter_map(|(n, o)| o.clean_text.clone().map(|c| (n.id, c))).collect();
            store.set_clean_text(id, &clean)?;
            let speakers: Vec<MeetingSpeaker> = bm.speakers.iter().map(|s| MeetingSpeaker { speaker_id: None, suggested_id: None, suggested_name: None, suggested_score: None, ..s.clone() }).collect();
            store.set_meeting_speakers(id, &speakers)?;
            if let Some(s) = &bm.summary {
                store.set_summary(s)?;
            }
            if !bm.highlights.is_empty() {
                store.set_setting(&format!("highlights:{id}"), &bm.highlights)?;
            }
            for t in &bm.tags {
                store.tag_meeting(id, t)?;
            }
            store.rebuild_chunks(id)?;
            Ok(())
        })();
        match res {
            Ok(()) => report.imported += 1,
            Err(e) => report.errors.push(format!("{}: {e}", bm.meeting.title)),
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hark_store::MeetingStatus;

    #[test]
    fn bundle_roundtrip_merges_without_duplicates() {
        let src_dir = tempfile::tempdir().unwrap();
        let src = Store::open(&src_dir.path().join("hark.db")).unwrap();
        let mut m = Meeting::new_recording("Budget", Some("zoom"));
        m.status = MeetingStatus::Ready;
        m.duration_ms = 4000;
        src.create_meeting(&m).unwrap();
        src.replace_segments(&m.id, &[NewSegment { start_ms: 0, end_ms: 1000, speaker: Some("Sam".into()), text: "um hello".into() }]).unwrap();
        let sid = src.segments(&m.id).unwrap()[0].id;
        src.set_clean_text(&m.id, &[(sid, "Hello.".into())]).unwrap();
        src.tag_meeting(&m.id, "finance").unwrap();
        src.set_setting(&format!("highlights:{}", m.id), &vec![500u64]).unwrap();
        std::fs::write(src.recordings_dir(&m.id).join("mix.wav"), b"RIFFfake").unwrap();

        let out = src_dir.path().join("export.hark");
        let manifest = export_bundle(&src, &[m.id.clone()], &out, true).unwrap();
        assert_eq!(manifest.meetings[0].media, vec!["mix.wav"]);

        let dst_dir = tempfile::tempdir().unwrap();
        let dst = Store::open(&dst_dir.path().join("hark.db")).unwrap();
        let r = import_bundle(&dst, &out).unwrap();
        assert_eq!(r, ImportReport { imported: 1, skipped_existing: 0, errors: vec![] });
        let got = dst.get_meeting(&m.id).unwrap().unwrap();
        assert_eq!(got.title, "Budget");
        let segs = dst.segments(&m.id).unwrap();
        assert_eq!(segs[0].clean_text.as_deref(), Some("Hello."));
        assert_eq!(dst.meeting_tags(&m.id).unwrap()[0].name, "finance");
        assert_eq!(std::fs::read(dst.recordings_dir(&m.id).join("mix.wav")).unwrap(), b"RIFFfake");
        assert_eq!(dst.search_chunks("hello", None, 5).unwrap().len(), 1);

        let r2 = import_bundle(&dst, &out).unwrap();
        assert_eq!(r2.skipped_existing, 1);
        assert_eq!(dst.list_meetings().unwrap().len(), 1);
    }
}
