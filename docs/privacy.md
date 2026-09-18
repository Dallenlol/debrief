# Privacy

Hark is built so that you never have to trust anyone with your meetings.

- **No cloud.** Recordings, transcripts, summaries and chats are files in one folder on your computer.
- **No account, no telemetry, no crash reporting.** Every network connection Hark makes is one you can see and turn off:
  - downloading AI models from Hugging Face when you ask;
  - fetching `latest.json` from GitHub releases on startup to check for updates (Settings > Updates; off = no request);
  - fetching the calendar .ics addresses you pasted (Settings > Calendar), on the schedule you set;
  - POSTing finished meetings to the webhook URL you entered (Settings > Webhook, off by default);
  - talking to a model server you configure yourself (off by default).
- **Attendee names** come from the meeting window on your own screen (Windows accessibility tree) or your calendar event. They are stored with the meeting and never sent anywhere unless you enable the webhook.
- **No bot.** Hark records what your computer already hears and shows. Nobody joins your call.
- **Only what you pick.** Every recording starts from a picker: the window or display for video, and - on Windows - just that application's audio or everything playing. Nothing else on your screen or speakers is captured.
- **Manual.** Detection only shows a popup. Recording starts when you click Record or press the hotkey. Speaker names are suggestions until you confirm. Share links exist only when you create them, on your network, while Hark is open.
- **Live notes stay local.** Captions and running notes shown during a call come from the same on-device models and are kept only in memory until the recording is processed.
- **Nothing lingers after a crash.** The screen recorder is tied to Hark's own process, so it cannot keep capturing if Hark dies; the interrupted recording is finished from disk on the next launch.

Recording other people may require their consent depending on where you are. Hark makes that easy to do properly, but it is your responsibility.
