use regex::Regex;
use serde::{Deserialize, Serialize};

/// A visible top-level window, as seen by the OS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowInfo {
    /// Lower-cased executable base name (Windows) or app owner name (macOS).
    pub process: String,
    pub title: String,
    /// Native window handle (Windows HWND); 0 when unknown.
    #[serde(default)]
    pub hwnd: isize,
    /// Owning process id; 0 when unknown.
    #[serde(default)]
    pub pid: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pattern {
    pub app: String,
    pub label: String,
    #[serde(default)]
    pub process: Vec<String>,
    #[serde(default)]
    pub title_any: Vec<String>,
    #[serde(default)]
    pub title_regex: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Detected {
    pub app: String,
    pub label: String,
    pub title: String,
    /// 1.0 for an app/window match, 0.5 for the audio-activity fallback.
    pub confidence: f32,
    /// Native handle of the matched window (Windows), 0 otherwise.
    #[serde(default)]
    pub hwnd: isize,
}

#[derive(Debug, Deserialize)]
struct PatternFile {
    #[serde(default)]
    pattern: Vec<Pattern>,
}

#[derive(Debug, thiserror::Error)]
pub enum PatternError {
    #[error("invalid patterns toml: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("invalid regex in pattern '{0}': {1}")]
    Regex(String, regex::Error),
}

struct Compiled {
    pattern: Pattern,
    process: Vec<String>,
    title_any: Vec<String>,
    regex: Option<Regex>,
}

pub struct PatternSet {
    items: Vec<Compiled>,
}

const BUILTIN: &str = include_str!("../patterns.toml");

impl PatternSet {
    pub fn builtin() -> PatternSet {
        Self::from_toml(BUILTIN).expect("built-in patterns.toml is valid")
    }

    pub fn from_toml(s: &str) -> Result<PatternSet, PatternError> {
        let file: PatternFile = toml::from_str(s)?;
        Self::from_patterns(file.pattern)
    }

    pub fn from_patterns(patterns: Vec<Pattern>) -> Result<PatternSet, PatternError> {
        let mut items = Vec::with_capacity(patterns.len());
        for p in patterns {
            let regex = match &p.title_regex {
                Some(r) => Some(Regex::new(r).map_err(|e| PatternError::Regex(p.app.clone(), e))?),
                None => None,
            };
            items.push(Compiled {
                process: p.process.iter().map(|s| s.to_lowercase()).collect(),
                title_any: p.title_any.iter().map(|s| s.to_lowercase()).collect(),
                regex,
                pattern: p,
            });
        }
        Ok(PatternSet { items })
    }

    /// User-defined patterns take priority over built-ins.
    pub fn merge_user(&mut self, extra: PatternSet) {
        let mut items = extra.items;
        items.append(&mut self.items);
        self.items = items;
    }

    pub fn patterns(&self) -> impl Iterator<Item = &Pattern> {
        self.items.iter().map(|c| &c.pattern)
    }

    /// First pattern (in priority order) that matches any window.
    pub fn match_windows(&self, windows: &[WindowInfo]) -> Option<Detected> {
        for c in &self.items {
            for w in windows {
                if c.matches(w) {
                    return Some(Detected {
                        app: c.pattern.app.clone(),
                        label: c.pattern.label.clone(),
                        title: w.title.clone(),
                        confidence: 1.0,
                        hwnd: w.hwnd,
                    });
                }
            }
        }
        None
    }
}

impl Compiled {
    fn matches(&self, w: &WindowInfo) -> bool {
        if w.title.trim().is_empty() {
            return false;
        }
        let proc_lc = w.process.to_lowercase();
        let proc_ok = self.process.is_empty() || self.process.iter().any(|p| proc_lc.contains(p.as_str()));
        if !proc_ok {
            return false;
        }
        let title_lc = w.title.to_lowercase();
        let any_ok = self.title_any.iter().any(|t| title_lc.contains(t.as_str()));
        let re_ok = self.regex.as_ref().map(|r| r.is_match(&w.title)).unwrap_or(false);
        if self.title_any.is_empty() && self.regex.is_none() {
            // Process-only pattern.
            return true;
        }
        any_ok || re_ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(p: &str, t: &str) -> WindowInfo {
        WindowInfo { process: p.into(), title: t.into(), hwnd: 0, pid: 0 }
    }

    #[test]
    fn matches_zoom_by_process_and_title() {
        let p = PatternSet::builtin();
        let d = p.match_windows(&[w("zoom.exe", "Zoom Meeting")]).unwrap();
        assert_eq!(d.app, "zoom");
        assert_eq!(d.confidence, 1.0);
    }

    #[test]
    fn browser_without_meet_title_does_not_match() {
        let p = PatternSet::builtin();
        assert!(p.match_windows(&[w("chrome.exe", "Inbox - Gmail")]).is_none());
    }

    #[test]
    fn meet_tab_matches() {
        let p = PatternSet::builtin();
        assert_eq!(p.match_windows(&[w("msedge.exe", "Meet - abc-defg-hij - Microsoft Edge")]).unwrap().app, "meet");
    }

    #[test]
    fn teams_regex_and_discord_voice() {
        let p = PatternSet::builtin();
        assert_eq!(p.match_windows(&[w("ms-teams.exe", "Weekly sync | Microsoft Teams")]).unwrap().app, "teams");
        assert_eq!(p.match_windows(&[w("discord.exe", "#general | Discord")]).unwrap().app, "discord");
        assert!(p.match_windows(&[w("discord.exe", "Discord")]).is_none());
    }

    #[test]
    fn user_patterns_take_priority_and_empty_titles_ignored() {
        let mut p = PatternSet::builtin();
        let user = PatternSet::from_toml(
            r#"[[pattern]]
app = "custom"
label = "My Tool"
process = ["chrome"]
title_any = ["Meet - "]
"#,
        )
        .unwrap();
        p.merge_user(user);
        assert_eq!(p.match_windows(&[w("chrome.exe", "Meet - xyz")]).unwrap().app, "custom");
        assert!(p.match_windows(&[w("zoom.exe", "   ")]).is_none());
    }

    #[test]
    fn browser_and_extra_apps() {
        let p = PatternSet::builtin();
        let cases = [
            ("chrome.exe", "Zoom Meeting - app.zoom.us - Google Chrome", "zoom"),
            ("firefox.exe", "Weekly sync | Microsoft Teams - teams.microsoft.com", "teams"),
            ("brave.exe", "Whereby - Meeting room", "whereby"),
            ("chrome.exe", "Jitsi Meet - Standup", "jitsi"),
            ("zoom.exe", "Reunión de Zoom", "zoom"),
            ("msedge.exe", "Meet – abc-defg-hij", "meet"),
            ("whatsapp.exe", "Voice call with Sam", "whatsapp"),
            ("signal.exe", "Signal call", "signal"),
            ("ringcentral.exe", "RingCentral Video", "ringcentral"),
            ("chrome.exe", "GoTo Meeting - app.goto.com/meeting/123", "gotomeeting"),
        ];
        for (proc_, title, app) in cases {
            assert_eq!(p.match_windows(&[w(proc_, title)]).map(|d| d.app), Some(app.to_string()), "{proc_} / {title}");
        }
        assert!(p.match_windows(&[w("whatsapp.exe", "WhatsApp")]).is_none());
        assert!(p.match_windows(&[w("chrome.exe", "Teams pricing - Google Chrome")]).is_none());
    }

    #[test]
    fn bad_regex_is_an_error() {
        let r = PatternSet::from_toml("[[pattern]]\napp='x'\nlabel='x'\ntitle_regex='('\n");
        assert!(matches!(r, Err(PatternError::Regex(_, _))));
    }
}
