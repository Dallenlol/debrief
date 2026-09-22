# FAQ

**Does it work offline?** Yes, completely, once the models are downloaded.

**Is it really free?** Yes. MIT licensed, no tiers, no upsell. Run it on as many machines as you like.

**How accurate is the transcription?** Whisper large-v3-turbo (GPU tier) is state of the art for English and most major languages. Smaller CPU models are good but make more mistakes; the AI cleanup pass fixes many of them. Live captions during the call use the small model and are provisional; the transcript you keep comes from the full pass after the call.

**How long does processing take after a call?** With the CUDA build, an 88-minute call takes about 10 minutes end to end (transcript, speakers, clean-up, summary, search index). On a CPU-only machine Hark measures how fast each speech model runs and picks the best one that finishes within the recording's own length; speaker detection runs across all cores in parallel and alongside transcription.

**Can it record only the meeting and not my music or notifications?** On Windows, yes: pick the meeting window and leave Audio on "This app only". Hark uses Windows' per-application audio capture, so other apps never reach the recording. Your mic is always included.

**What happens if my headset disconnects or the app crashes mid-call?** Hark switches to the current audio device and keeps recording. If the app dies, the recording is on disk already and is finished automatically the next time Hark starts.

**Why didn't it detect my meeting app?** Add a pattern under Settings > Detection, or open an issue with the window title. The audio-activity fallback catches most calls anyway.

**Can I use it for in-person meetings?** Yes: start a recording manually and pick "Everything playing" (or any window; only the mic matters). Speaker identification works from the mic alone. You can also record on your phone and import the file later.

**Can I import an old recording?** Yes. Library > Import takes any audio or video file (mp3, m4a, wav, mp4, mov, ...) and treats it like a recording: transcript, speakers, summary, search. The file's date becomes the meeting date.

**How do I get a follow-up email or a subtitle file?** On the meeting page: Summary > Follow-up email drafts a short recap with decisions and owned action items (editable, Copy). Export copies or saves the transcript as .txt, .srt or .md.

**How do I move my library to a new computer?** Settings > Data > Export everything, copy the `.hark` file, then Import on the other machine. Or move the whole data folder.

**Does it work with Zoom's or Teams' own recording?** Independently. Hark records what your computer plays and your mic; it doesn't care what the meeting app does.

**Why is the Windows installer unsigned?** Code signing certificates cost money. Verify the SHA-256 on the release page if that matters to you, or build from source.

## Where do attendee names come from?

On Windows, Hark reads the names shown in the meeting app's own window (Zoom roster, Teams participant list, Meet tiles) through the accessibility tree, the same way a screen reader would. If you added a calendar source, the event's attendees are used too. Names are only suggestions in the speaker rename box; Hark never assigns them by itself. macOS support for window scraping is planned.

## How do updates work?

On startup Hark fetches one small file (`latest.json`) from GitHub releases. If a newer signed build exists you get a banner; nothing installs until you click. Turn the check off in Settings > Updates.

## Does the webhook send my data somewhere?

Only if you turn it on and enter a URL. Then each finished meeting (title, attendees, summary, transcript) is POSTed to that address once. There is a "Send test" button so you can see the shape of the payload first.

## Hark split one person into two speakers. How do I fix it?

On the meeting page, the **Speakers** list shows every voice it found. Give the extra row the same name as the real person - type it, or pick the person under "Same person as". Both rows become one: the transcript, talk time, summary and search all update, and the two voiceprints are blended so the next meeting recognises that person more reliably.

The reverse (one row that is really two people) needs a re-run: **Re-run > Speaker detection only**, which re-clusters the voices from the audio.
