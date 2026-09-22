# Changelog

## Unreleased

- **Speakers panel.** The meeting page lists everyone Hark heard, with a name field, the voice-memory suggestion and how much each said. Naming two rows the same person **merges** them - one voice split into "Speaker 2" and "Speaker 5" becomes one person everywhere (transcript, talk time, summary, search), and the voiceprints are blended so the next meeting recognises them. "Same person as" does the same from a picker. Merging into a row still called "Speaker 3" no longer invents a person called "Speaker 3" in voice memory.

## 0.3.0 - 2026-09-15

- **Pick what to record, every time.** Record now, the hotkey, the tray and the call popup all open the same picker: choose the meeting window (or a display), and on Windows Hark captures **only that app's audio** plus your mic - music, notifications and other calls stay out. "Everything playing" is one click away.
- **Live transcript and live notes.** Open the recording from the Library while it runs: captions land as people talk (the current line refines in place), and running notes - what's being discussed, decisions and action items so far - update about a minute after new lines arrive, at most every two minutes.
- **Live captions you can read.** The live pass now transcribes whole utterances (refined every 2 s, committed on a pause), carries the previous text as context so names stay consistent, never transcribes silence (which whisper turned into invented sentences), drops "thanks for watching"-style hallucinations, and locks the language after the first detection.
- **Recordings survive device changes.** When Windows invalidates the audio device mid-call (headset unplugged, output switched, the call app releasing the device), Hark reopens the current devices and keeps going; a dead stream can no longer hang Stop or freeze the window. Recordings left in "recording" by a crash or hang are finished automatically on the next launch from the audio on disk.
- **Import a recording.** Library > Import (or drop in the empty state): any audio or video file - a phone voice memo, an old Zoom .mp4 - becomes a meeting and goes through transcription, speakers, clean-up, summary and search indexing. The file's date becomes the meeting date; video is kept when the player can show it.
- **Follow-up email.** Summary tab > Follow-up email drafts a short recap with decisions and owned action items from the notes (local model), in an editable box with Copy.
- **Export.** Copy the transcript or summary; save the transcript as .txt, .srt subtitles, or notes + transcript as .md.
- **Talk time.** A bar under the title shows who spoke how much; it follows the names you assign.
- **Fix: renamed speakers now reach Ask Hark.** Naming "Speaker 2" as Sarah rebuilds the search index, so "what did Sarah agree to?" finds her lines, and an existing summary is rewritten with the names a few seconds after you stop renaming. Previously chat and the summary kept the old labels.
- **Fix: first run.** "Open Hark" at the end of onboarding bounced back to onboarding until the app was restarted.
- **Fix: playback.** The player could not load the recording (asset scope did not match the data folder), so meetings played as 0:00.
- **Fix: CPU installer on GPU machines.** The plain (non-CUDA) build picked the GPU-sized models whenever an NVIDIA card was present and then ran them on the CPU; it now sizes for the CPU.
- **Build: portable CPU code.** whisper.cpp was compiled for the build machine's CPU (AVX-512 here), which crashes on CPUs without it; every build now targets AVX2 (2013+). CI cache key bumped.
- **Processing is several times faster.** Speaker detection now uses all cores in parallel slices (an 88-minute call: 10 min → 1.5 min) and runs while whisper transcribes instead of after it. On CPU-only machines Hark measures each speech model's real speed and picks the best one that finishes within the recording's length; the CPU tiers default to whisper-small so a long call never turns into a multi-hour wait.
- **Fix: long calls no longer come back with hundreds of "speakers".** Diarization clusters with almost no speech are folded into the nearest voice and the count is capped at eight; an 88-minute Teams call went from 288 labels to 8.
- **Fix: the AI engines stayed loaded through long jobs.** The idle unloader could drop the language model in the middle of clean-up on a long meeting, silently leaving no clean text, summary or search index.
- **Fix: the CUDA installer really uses the GPU.** The bundle script could ship a CPU `ggml.dll` next to the CUDA build; it now collects only the current build's libraries (94 tokens/s vs 9 on an RTX 3080).
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
