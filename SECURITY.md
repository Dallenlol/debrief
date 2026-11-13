# Security

Debrief runs on your computer and has no servers of its own. The attack surface is small but not zero:

- The LAN share server (`Settings > Data > Share links`) listens on all interfaces while a link is enabled. Links are 128-bit random tokens; anyone on your network with the link can view that recording. Disable links when done.
- Model files are downloaded over HTTPS and verified by SHA-256 before use.
- The optional OpenAI-compatible endpoint sends transcript text to whatever server you configure. That is your choice and your server.
- Optional Cloud AI (off by default) sends transcript text, participant names and summaries to Anthropic, OpenAI or OpenRouter with your API key. The key is stored in Debrief's settings database (`hark.db` in the data folder) without encryption, so protect that folder like any other file holding a password, and revoke the key at the provider if the computer is lost.
- Update checks and installers come from this repository's GitHub releases; updates are signed and verified against the public key built into the app.

To report a vulnerability, use GitHub's private vulnerability reporting: Security tab > Report a vulnerability. Please do not file public issues for security reports.
