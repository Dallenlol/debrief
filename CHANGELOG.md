# Changelog

## 0.3.0 - 2026-09-15

- **Import a recording.** Library > Import (or drop in the empty state): any audio or video file - a phone voice memo, an old Zoom .mp4 - becomes a meeting and goes through transcription, speakers, clean-up, summary and search indexing. The file's date becomes the meeting date; video is kept when the player can show it.
- **Follow-up email.** Summary tab > Follow-up email drafts a short recap with decisions and owned action items from the notes (local model), in an editable box with Copy.
- **Export.** Copy the transcript or summary; save the transcript as .txt, .srt subtitles, or notes + transcript as .md.
- **Talk time.** A bar under the title shows who spoke how much; it follows the names you assign.
- **Fix: renamed speakers now reach Ask Hark.** Naming "Speaker 2" as Sarah rebuilds the search index, so "what did Sarah agree to?" finds her lines. Previously chat kept seeing the old label.
- Build: the workspace test run no longer breaks when the sherpa-onnx download cache is gone.

## 0.2.0 - 2026-09-14

- **Long meetings summarise fully.** Transcripts beyond ~25 minutes are condensed part by part before the template runs, instead of dropping the middle.
- **Semantic search in Ask Hark.** A small local embedding model (BGE small, 37 MB) is fused with keyword search, so "were they hesitant about price?" finds the right passage.
- **Auto-titles.** Meetings are named from their content once processed; a title you typed is never overwritten.
- **Attendee names.** On Windows, Hark reads participant names from the Zoom / Teams / Meet window (accessibility tree) and offers them when you name a speaker. Never auto-assigned.
- **Calendar context.** Add private .ics links (Google, Outlook) or files; recordings take the event's title and attendees, the Record popup shows the event, and the library has a Today strip.
- **Re-run anything.** Per-stage re-runs (transcribe with another model, speakers, clean-up, search index, summary), a visible error with Retry when a step fails.
- **More apps.** Zoom / Teams / Webex / GoTo in a browser tab, Whereby, Jitsi, Around, Skype, RingCentral, BlueJeans, Gather, Butter, WhatsApp / Telegram / Signal calls, localized Zoom titles.
- **Spoken language** setting (auto-detect or 30 languages); clean-up, summaries and chat follow the transcript's language.
- **Screen picker** on the Record popup (any display or window; the meeting window is preselected); the popup opens on the monitor the meeting is on.
- **Webhook** (opt-in): POST each finished meeting to Zapier / n8n / Make / your script.
- **Signed auto-updates** with a one-click install banner and Settings > Check for updates.
- **Lighter first run.** Start with the essentials (~0.6 GB); the big models finish in the background and Hark catches up on clean-up and summaries by itself.
- Live captions refine in place instead of flickering; keyboard and screen-reader pass on the player, popup and dialogs.

## 0.1.0 - 2026-09-14

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
