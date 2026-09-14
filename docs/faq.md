# FAQ

**Does it work offline?** Yes, completely, once the models are downloaded.

**Is it really free?** Yes. MIT licensed, no tiers, no upsell. Run it on as many machines as you like.

**How accurate is the transcription?** Whisper large-v3-turbo (GPU tier) is state of the art for English and most major languages. Smaller CPU models are good but make more mistakes; the AI cleanup pass fixes many of them.

**Why didn't it detect my meeting app?** Add a pattern under Settings > Detection, or open an issue with the window title. The audio-activity fallback catches most calls anyway.

**Can I use it for in-person meetings?** Yes: start a recording manually. Speaker identification works from the mic alone.

**How do I move my library to a new computer?** Settings > Data > Export everything, copy the `.hark` file, then Import on the other machine. Or move the whole data folder.

**Does it work with Zoom's or Teams' own recording?** Independently. Hark records what your computer plays and your mic; it doesn't care what the meeting app does.

**Why is the Windows installer unsigned?** Code signing certificates cost money. Verify the SHA-256 on the release page if that matters to you, or build from source.

## Where do attendee names come from?

On Windows, Hark reads the names shown in the meeting app's own window (Zoom roster, Teams participant list, Meet tiles) through the accessibility tree, the same way a screen reader would. If you added a calendar source, the event's attendees are used too. Names are only suggestions in the speaker rename box; Hark never assigns them by itself. macOS support for window scraping is planned.

## How do updates work?

On startup Hark fetches one small file (`latest.json`) from GitHub releases. If a newer signed build exists you get a banner; nothing installs until you click. Turn the check off in Settings > Updates.

## Does the webhook send my data somewhere?

Only if you turn it on and enter a URL. Then each finished meeting (title, attendees, summary, transcript) is POSTed to that address once. There is a "Send test" button so you can see the shape of the payload first.
