//! Attendee names from a meeting window's accessibility tree.
//!
//! `scrape_window` (Windows, UI Automation) returns every element name under
//! the window; `extract_names` turns that noisy list into people. Names are
//! only ever *suggested* to the user - see the speaker rename picker.

/// Words that mark a control, not a person. Any token matching one drops the candidate.
const UI_WORDS: &[&str] = &[
    "mute", "unmute", "muted", "video", "audio", "share", "screen", "chat", "participants", "record", "recording",
    "reactions", "apps", "leave", "end", "more", "settings", "view", "gallery", "speaker", "meeting", "call", "join",
    "camera", "microphone", "raise", "hand", "present", "presenting", "captions", "whiteboard", "breakout", "rooms",
    "security", "tools", "notes", "polls", "timer", "invite", "search", "close", "minimize", "maximize", "back",
    "next", "send", "message", "everyone", "zoom", "teams", "google", "microsoft", "meet", "discord", "slack",
    "webex", "chrome", "edge", "firefox", "tab", "new", "window", "people", "activities", "turn", "off", "on",
    "options", "menu", "transcript", "live", "stream", "info", "details", "layout", "pin", "spotlight", "remove",
    "ask", "copy", "link", "admit", "deny", "lobby", "waiting", "room", "return", "fullscreen", "exit", "dial",
    "phone", "test", "computer", "sound", "hide", "show", "self", "stop", "start", "volume", "speakers", "unknown",
    "button", "toolbar", "panel", "list", "item", "dialog", "header", "footer", "navigation", "scroll", "bar",
    "toggle", "open", "expand", "collapse", "add", "cancel", "ok", "yes", "no", "done", "apply", "save", "edit",
    "delete", "help", "about", "sign", "account", "profile", "status", "online", "away", "busy", "connected",
    "connecting", "reconnecting", "poor", "connection", "quality", "hd", "sd", "you", "me", "host", "co-host",
    "guest", "organizer", "attendee", "presenter", "speaking", "talking", "typing", "is", "are", "and", "the",
    "in", "of", "to", "for", "with", "from", "your", "this", "that", "all", "none", "voice", "channel", "server",
    "text", "friends", "direct", "messages", "inbox", "home", "library", "calendar", "files", "activity", "mail",
    "gemini", "companion", "ai", "assistant", "notetaker", "recorder", "otter", "fathom", "hark", "fireflies",
];

/// Suffixes the apps append to a name: "(Host, me)", ", Muted", "is presenting" ...
const STRIP_AFTER: &[&str] = &[" (", ", ", " - ", " – ", " is presenting", " is speaking", " speaking", " muted", " unmuted"];

/// People's names among raw accessibility-element names for `app`.
pub fn extract_names(app: &str, raw: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for r in raw {
        if let Some(name) = clean_candidate(app, r) {
            if !out.iter().any(|o| o.eq_ignore_ascii_case(&name)) {
                out.push(name);
            }
        }
        if out.len() >= 50 {
            break;
        }
    }
    out
}

fn clean_candidate(app: &str, raw: &str) -> Option<String> {
    let mut s = raw.trim();
    if s.is_empty() || s.chars().count() > 60 || s.contains(['\n', '\t']) {
        return None;
    }
    // Meet: "Sarah Chen (You)", "Marcus Webb is presenting"; Teams: "Marcus Webb, Muted, Attendee";
    // Zoom: "John Smith (Host, me)"; Discord: "Marcus, speaking".
    let mut had_marker = s.contains("(You)") || s.contains("(you)") || s.contains("(me)") || s.contains("(Me)") || s.contains("(Host") || s.contains("(Guest)");
    for sep in STRIP_AFTER {
        if let Some(i) = s.find(sep) {
            s = s[..i].trim();
            had_marker = true;
        }
    }
    let tokens: Vec<&str> = s.split_whitespace().collect();
    // Single-word names only when the app annotated them (", speaking", "(You)"), otherwise
    // channel and button labels would slip through.
    // Discord/Slack windows are full of server and channel names, so there a
    // candidate must carry an annotation to count at all.
    let chatty = matches!(app, "discord" | "slack");
    if chatty && !had_marker {
        return None;
    }
    if tokens.is_empty() || tokens.len() > 4 || (tokens.len() == 1 && !chatty) {
        return None;
    }
    for t in &tokens {
        let lower = t.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
        if lower.is_empty() || UI_WORDS.contains(&lower.as_str()) {
            return None;
        }
        if t.chars().any(|c| c.is_ascii_digit() || matches!(c, '@' | '/' | '\\' | ':' | ';' | '|' | '#' | '_' | '[' | ']' | '{' | '}' | '<' | '>' | '=' | '+' | '*' | '&' | '%' | '$' | '!' | '?')) {
            return None;
        }
        if !t.chars().next().is_some_and(|c| c.is_alphabetic() && (c.is_uppercase() || !c.is_ascii())) {
            return None;
        }
    }
    // At least one token longer than an initial.
    if !tokens.iter().any(|t| t.chars().filter(|c| c.is_alphabetic()).count() >= 2) {
        return None;
    }
    Some(tokens.join(" "))
}

/// Every element name under the window (Windows UI Automation). Empty on other platforms.
pub fn scrape_window(hwnd: isize) -> Vec<String> {
    imp::scrape(hwnd)
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
    use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation, TreeScope_Descendants, UIA_NamePropertyId};

    const MAX_ELEMENTS: i32 = 3000;

    pub fn scrape(hwnd: isize) -> Vec<String> {
        if hwnd == 0 {
            return Vec::new();
        }
        // SAFETY: plain COM calls on a live HWND; every result is checked.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let Ok(uia) = CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER) else {
                return Vec::new();
            };
            let Ok(root) = uia.ElementFromHandle(HWND(hwnd as *mut _)) else { return Vec::new() };
            let Ok(cond) = uia.CreateTrueCondition() else { return Vec::new() };
            let Ok(cache) = uia.CreateCacheRequest() else { return Vec::new() };
            let _ = cache.AddProperty(UIA_NamePropertyId);
            let mut out = Vec::new();
            // Chromium/Electron apps (Discord, Teams, Meet in a browser) only build their
            // accessibility tree after a client asks for it, so a first query can come back
            // nearly empty; ask again after a short pause.
            for attempt in 0..3 {
                out.clear();
                let Ok(all) = root.FindAllBuildCache(TreeScope_Descendants, &cond, &cache) else { return Vec::new() };
                let n = all.Length().unwrap_or(0).min(MAX_ELEMENTS);
                for i in 0..n {
                    if let Ok(el) = all.GetElement(i) {
                        if let Ok(name) = el.CachedName() {
                            let s = name.to_string();
                            if !s.trim().is_empty() {
                                out.push(s);
                            }
                        }
                    }
                }
                if out.len() >= 8 || attempt == 2 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(400));
            }
            out
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn scrape(_hwnd: isize) -> Vec<String> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn zoom_roster_and_tiles() {
        let raw = v(&["Mute My Audio", "Start Video", "Participants (3)", "John Smith (Host, me)", "Priya Patel", "Priya Patel", "Marcus Webb (Guest)", "Share Screen", "Gallery View", "Zoom Meeting", "iPhone 12", "Leave"]);
        assert_eq!(extract_names("zoom", &raw), vec!["John Smith", "Priya Patel", "Marcus Webb"]);
    }

    #[test]
    fn teams_roster_rows() {
        let raw = v(&["Marcus Webb, Muted, Attendee", "Sarah Chen, Organizer", "Turn camera on", "People", "Raise hand", "Ana-Maria Ionescu, Unmuted", "Microsoft Teams"]);
        assert_eq!(extract_names("teams", &raw), vec!["Marcus Webb", "Sarah Chen", "Ana-Maria Ionescu"]);
    }

    #[test]
    fn meet_labels() {
        let raw = v(&["Sarah Chen (You)", "Marcus Webb is presenting", "Turn off microphone", "Present now", "Meet - abc-defg-hij", "Show everyone", "Priya Patel", "Priya", "Gemini notetaker"]);
        assert_eq!(extract_names("meet", &raw), vec!["Sarah Chen", "Marcus Webb", "Priya Patel"]);
    }

    #[test]
    fn discord_allows_single_names_and_skips_channels() {
        let raw = v(&["Marcus, speaking", "Priya, muted", "Priya", "General", "Voice Connected", "Disconnect", "marcus"]);
        assert_eq!(extract_names("discord", &raw), vec!["Marcus", "Priya"]);
    }

    #[test]
    fn rejects_noise() {
        let raw = v(&["", "OK", "Sign in", "user@example.com", "12:30 PM", "Recording in progress", "A B", "J. R. R. Tolkien", "Émile Zola", &"Word ".repeat(20)]);
        assert_eq!(extract_names("zoom", &raw), vec!["J. R. R. Tolkien", "Émile Zola"]);
    }
}

#[cfg(all(test, windows))]
mod live_tests {
    /// Scrapes the first visible window whose title contains `HARK_SCRAPE_TITLE` and prints what UIA sees.
    #[test]
    #[ignore]
    fn scrape_live_window() {
        let want = std::env::var("HARK_SCRAPE_TITLE").unwrap_or_else(|_| "Claude".into());
        let w = crate::list_visible_windows().into_iter().find(|w| w.title.contains(&want)).expect("window");
        let raw = crate::scrape_window(w.hwnd);
        println!("{} elements under '{}' ({})", raw.len(), w.title, w.process);
        for r in raw.iter().take(40) {
            println!("  {r:?}");
        }
        println!("names: {:?}", crate::extract_names("zoom", &raw));
        assert!(!raw.is_empty());
    }
}
