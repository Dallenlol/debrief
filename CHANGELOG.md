# Changelog

## 0.1.0 - unreleased

First public build.

- Detects Zoom, Teams, Google Meet, Webex, Discord, Slack huddles, FaceTime and GoToMeeting, plus an audio-activity fallback. Shows a Record popup; never records on its own.
- Records microphone and system audio as separate tracks, optional screen video, crash-safe writes.
- Live captions while recording; full transcript after, with speaker labels and "Me" detection.
- Speaker naming with voice memory: name someone once, get a confirmable suggestion next time.
- AI cleanup of messy transcripts (bad mics, broken English, filler), raw transcript always kept.
- Summaries from editable templates: general, sales call, client discovery, 1:1, standup, interview.
- Ask Hark: chat over one meeting or your whole library, with clickable timestamp citations.
- Folders, tags, search, highlights, clips.
- Share links on your network, `.hark` bundles and standalone HTML export, import with de-duplication, movable data folder.
- Bundled llama.cpp (Qwen3 1.7B / 4B / 8B by hardware tier) or any OpenAI-compatible endpoint.
