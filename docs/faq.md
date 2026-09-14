# FAQ

**Does it work offline?** Yes, completely, once the models are downloaded.

**Is it really free?** Yes. MIT licensed, no tiers, no upsell. Run it on as many machines as you like.

**How accurate is the transcription?** Whisper large-v3-turbo (GPU tier) is state of the art for English and most major languages. Smaller CPU models are good but make more mistakes; the AI cleanup pass fixes many of them.

**Why didn't it detect my meeting app?** Add a pattern under Settings > Detection, or open an issue with the window title. The audio-activity fallback catches most calls anyway.

**Can I use it for in-person meetings?** Yes: start a recording manually. Speaker identification works from the mic alone.

**How do I move my library to a new computer?** Settings > Data > Export everything, copy the `.hark` file, then Import on the other machine. Or move the whole data folder.

**Does it work with Zoom's or Teams' own recording?** Independently. Hark records what your computer plays and your mic; it doesn't care what the meeting app does.

**Why is the Windows installer unsigned?** Code signing certificates cost money. Verify the SHA-256 on the release page if that matters to you, or build from source.
