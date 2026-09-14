# Security

Hark runs entirely on your computer. The attack surface is small but not zero:

- The LAN share server (`Settings > Data > Share links`) listens on all interfaces while a link is enabled. Links are 128-bit random tokens; anyone on your network with the link can view that recording. Disable links when done.
- Model files are downloaded over HTTPS and verified by SHA-256 before use.
- The optional OpenAI-compatible endpoint sends transcript text to whatever server you configure. That is your choice and your server.

To report a vulnerability, open a private security advisory on GitHub (Security tab > Report a vulnerability). Please do not file public issues for security reports.
