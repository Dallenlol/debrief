# Signing and notarizing the macOS builds

Unsigned Macs builds work, but macOS blocks them on first open ("can't be opened" or "is damaged"; right-click > Open gets past it) and privacy permissions (microphone, system audio, screen recording) can be flaky for unsigned apps. Signing and notarizing fixes both. The release workflow does it automatically once these repository secrets exist, and builds unsigned when they don't.

## What you need

An Apple Developer Program membership (US$99/year) at developer.apple.com.

## One-time setup

1. **Developer ID certificate.** In Xcode (Settings > Accounts > Manage Certificates) or at developer.apple.com > Certificates, create a **Developer ID Application** certificate. Export it from Keychain Access as a `.p12` with a password.
2. **App-specific password.** At account.apple.com > Sign-In and Security > App-Specific Passwords, create one for notarization.
3. **Repository secrets** (GitHub > Settings > Secrets and variables > Actions):

| Secret | Value |
|---|---|
| `APPLE_CERTIFICATE` | `base64 -i DeveloperID.p12 \| pbcopy`, then paste |
| `APPLE_CERTIFICATE_PASSWORD` | the `.p12` export password |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Your Name (TEAMID)` (`security find-identity -v -p codesigning`) |
| `APPLE_ID` | your Apple ID email |
| `APPLE_PASSWORD` | the app-specific password |
| `APPLE_TEAM_ID` | the 10-character team ID (developer.apple.com > Membership) |

The next tagged release signs both Mac builds with the hardened runtime, using `src-tauri/entitlements.plist`, and notarizes them, so they open with a double-click.
