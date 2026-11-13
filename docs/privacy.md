# Privacy

Debrief is built so that you never have to trust anyone with your meetings.

- **Local by default.** Recordings, transcripts, summaries and chats are files in one folder on your computer. Out of the box, all AI (transcription, speakers, summaries, chat) runs on your machine.
- **No account, no telemetry, no crash reporting.** Every network connection Debrief makes is one you can see and turn off:
  - downloading AI models from Hugging Face and GitHub when you ask;
  - fetching `latest.json` from GitHub releases on startup to check for updates (Settings > Updates; off = no request);
  - fetching the calendar .ics addresses you pasted (Settings > Calendar), on the schedule you set;
  - POSTing finished meetings to the webhook URL you entered (Settings > Webhook, off by default);
  - talking to a model server you configure yourself (off by default);
  - **Cloud AI, only if you turn it on** (Settings > AI assistant > Cloud AI, off by default): transcript text, participant names, the summary and your questions go to the provider you pick (Anthropic Claude, OpenAI or OpenRouter) under your own API key and that provider's privacy terms. This covers summaries, titles, clean-up, follow-ups, Ask and the live notes during a call. Audio never leaves your computer; transcription stays on-device.
- **Attendee names** come from the meeting window on your own screen (Windows accessibility tree) or your calendar event. They are stored with the meeting and only leave your computer if you enable the webhook or Cloud AI.
- **No bot.** Debrief records what your computer already hears and shows. Nobody joins your call.
- **Only what you pick.** Every recording starts from a picker: the window or display for video, and - on Windows - just that application's audio or everything playing. Nothing else on your screen or speakers is captured.
- **Manual.** Detection only shows a popup. Recording starts when you click Record or press the hotkey. Speaker names are suggestions until you confirm. Share links exist only when you create them, on your network, while Debrief is open.
- **Live notes stay local** unless Cloud AI is on. Captions always come from the on-device speech model; the running notes come from whichever AI assistant you chose. Both are kept only in memory until the recording is processed.
- **Nothing lingers after a crash.** The screen recorder is tied to Debrief's own process, so it cannot keep capturing if Debrief dies; the interrupted recording is finished from disk on the next launch.

Recording other people may require their consent depending on where you are. Debrief makes that easy to do properly, but it is your responsibility.
