//! Summary templates: built-ins, placeholder rendering, generation and
//! tolerant parsing of the model's markdown into structured sections.

use crate::backend::{complete, ChatMessage, GenOptions, LlmBackend, LlmError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuiltinTemplate {
    pub id: &'static str,
    pub name: String,
    pub description: String,
    pub body: String,
}

const RAW: &[(&str, &str)] = &[
    ("general", include_str!("../templates/general.md")),
    ("sales-call", include_str!("../templates/sales-call.md")),
    ("client-discovery", include_str!("../templates/client-discovery.md")),
    ("one-on-one", include_str!("../templates/one-on-one.md")),
    ("standup", include_str!("../templates/standup.md")),
    ("interview", include_str!("../templates/interview.md")),
];

/// Parse `# Name`, `> description`, body.
pub fn parse_template_file(id: &'static str, src: &str) -> BuiltinTemplate {
    let mut name = id.to_string();
    let mut description = String::new();
    let mut body_lines = Vec::new();
    let mut header = true;
    for line in src.lines() {
        if header {
            if let Some(n) = line.strip_prefix("# ") {
                name = n.trim().to_string();
                continue;
            }
            if let Some(d) = line.strip_prefix("> ") {
                description = d.trim().to_string();
                continue;
            }
            if line.trim().is_empty() {
                continue;
            }
            header = false;
        }
        body_lines.push(line);
    }
    BuiltinTemplate { id, name, description, body: body_lines.join("\n").trim().to_string() }
}

pub fn builtin_templates() -> Vec<BuiltinTemplate> {
    RAW.iter().map(|(id, src)| parse_template_file(id, src)).collect()
}

/// Variables available to templates.
#[derive(Debug, Clone, Default)]
pub struct TemplateVars {
    pub title: String,
    pub date: String,
    pub duration: String,
    pub participants: String,
    pub transcript: String,
    pub highlights: String,
}

/// Replace `{{name}}` placeholders. Unknown placeholders are left as-is.
pub fn render(body: &str, vars: &TemplateVars) -> String {
    let pairs = [
        ("{{title}}", vars.title.as_str()),
        ("{{date}}", vars.date.as_str()),
        ("{{duration}}", vars.duration.as_str()),
        ("{{participants}}", vars.participants.as_str()),
        ("{{transcript}}", vars.transcript.as_str()),
        ("{{highlights}}", vars.highlights.as_str()),
    ];
    let mut out = body.to_string();
    for (k, v) in pairs {
        out = out.replace(k, v);
    }
    out
}

/// A transcript line for prompts: `[m:ss] Speaker: text`.
pub fn transcript_line(start_ms: i64, speaker: Option<&str>, text: &str) -> String {
    let stamp = fmt_stamp(start_ms);
    match speaker {
        Some(s) => format!("[{stamp}] {s}: {text}"),
        None => format!("[{stamp}] {text}"),
    }
}

pub fn fmt_stamp(ms: i64) -> String {
    let total = (ms.max(0) / 1000) as u64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Parse `m:ss` or `h:mm:ss` into milliseconds.
pub fn parse_stamp(s: &str) -> Option<i64> {
    let parts: Vec<u64> = s.trim().split(':').map(|p| p.trim().parse::<u64>().ok()).collect::<Option<_>>()?;
    let secs = match parts.as_slice() {
        [m, s] => m * 60 + s,
        [h, m, s] => h * 3600 + m * 60 + s,
        _ => return None,
    };
    Some((secs * 1000) as i64)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct KeyMoment {
    pub ms: Option<i64>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Structured {
    pub summary: String,
    pub action_items: Vec<String>,
    pub decisions: Vec<String>,
    pub open_questions: Vec<String>,
    pub key_moments: Vec<KeyMoment>,
    /// Every section by lower-cased heading, for templates with custom sections.
    pub sections: BTreeMap<String, String>,
}

/// Split markdown on `## ` headings and pull out the well-known sections.
pub fn parse_sections(markdown: &str) -> Structured {
    let mut sections: BTreeMap<String, String> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    let mut buf = String::new();
    let flush = |current: &Option<String>, buf: &mut String, sections: &mut BTreeMap<String, String>, order: &mut Vec<String>| {
        if let Some(h) = current {
            let text = buf.trim().to_string();
            if !order.contains(h) {
                order.push(h.clone());
            }
            sections.insert(h.clone(), text);
        }
        buf.clear();
    };
    for line in markdown.lines() {
        let trimmed = line.trim();
        let heading = trimmed.strip_prefix("## ").or_else(|| trimmed.strip_prefix("# ")).or_else(|| trimmed.strip_prefix("### "));
        if let Some(h) = heading {
            flush(&current, &mut buf, &mut sections, &mut order);
            current = Some(h.trim().trim_end_matches(':').to_lowercase());
        } else if current.is_some() {
            buf.push_str(line);
            buf.push('\n');
        } else if !trimmed.is_empty() {
            // Text before the first heading counts as the summary.
            current = Some("summary".into());
            buf.push_str(line);
            buf.push('\n');
        }
    }
    flush(&current, &mut buf, &mut sections, &mut order);

    let get = |names: &[&str]| -> String { names.iter().find_map(|n| sections.get(*n).cloned()).unwrap_or_default() };
    let bullets = |s: &str| -> Vec<String> {
        s.lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .map(|l| l.trim_start_matches(['-', '*', '+']).trim().trim_start_matches("[ ]").trim_start_matches("[x]").trim().to_string())
            .filter(|l| !l.is_empty() && !l.eq_ignore_ascii_case("none") && !l.eq_ignore_ascii_case("none recorded.") && !l.eq_ignore_ascii_case("none."))
            .collect()
    };
    let key_moments = bullets(&get(&["key moments", "highlights", "moments"]))
        .into_iter()
        .map(|l| {
            let (ms, text) = match (l.find('['), l.find(']')) {
                (Some(0), Some(e)) => (parse_stamp(&l[1..e]), l[e + 1..].trim().trim_start_matches(['-', ':']).trim().to_string()),
                _ => (None, l),
            };
            KeyMoment { ms, text }
        })
        .collect();
    Structured {
        summary: get(&["summary", "overview", "tl;dr"]),
        action_items: bullets(&get(&["action items", "next steps", "actions", "to-do", "todos"])),
        decisions: bullets(&get(&["decisions"])),
        open_questions: bullets(&get(&["open questions", "questions", "follow-ups", "follow ups"])),
        key_moments,
        sections,
    }
}

pub struct SummaryResult {
    pub markdown: String,
    pub structured: Structured,
}

pub const SUMMARY_SYSTEM: &str = "You write precise, faithful meeting notes from transcripts. Follow the requested section structure exactly and output markdown only. Write in the same language as the transcript.";

/// Render the template and ask the model. Transcripts longer than `max_chars`
/// are first condensed block-by-block (see `chunked`) so nothing is dropped.
pub fn generate(backend: &dyn LlmBackend, template_body: &str, vars: &TemplateVars, max_chars: usize) -> Result<SummaryResult, LlmError> {
    generate_with_progress(backend, template_body, vars, max_chars, |_, _| {})
}

/// `on_progress(done, total)` fires once per condensed block (never for short transcripts).
pub fn generate_with_progress(
    backend: &dyn LlmBackend,
    template_body: &str,
    vars: &TemplateVars,
    max_chars: usize,
    on_progress: impl FnMut(usize, usize),
) -> Result<SummaryResult, LlmError> {
    let mut v = vars.clone();
    if v.transcript.len() > max_chars {
        // Blocks are ~5/6 of the budget so the joined notes stay well under it.
        let notes = crate::chunked::notes_for_blocks(backend, &v.transcript, max_chars * 5 / 6, on_progress)?;
        v.transcript = if notes.len() > max_chars { truncate_middle(&notes, max_chars) } else { notes };
    }
    let prompt = render(template_body, &v);
    let msgs = [ChatMessage::system(SUMMARY_SYSTEM), ChatMessage::user(prompt)];
    let reply = complete(backend, &msgs, &GenOptions { max_tokens: 1200, temperature: 0.2, json: false })?;
    let markdown = reply.trim().trim_start_matches("```markdown").trim_start_matches("```").trim_end_matches("```").trim().to_string();
    let structured = parse_sections(&markdown);
    Ok(SummaryResult { markdown, structured })
}

/// Keep the beginning and end of a long text, dropping whole lines from the middle.
pub fn truncate_middle(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }
    let lines: Vec<&str> = text.lines().collect();
    let mut head = Vec::new();
    let mut tail = Vec::new();
    let (mut used, budget) = (0usize, max_chars.saturating_sub(64));
    let (mut i, mut j) = (0usize, lines.len());
    while i < j && used < budget {
        if head.len() <= tail.len() {
            used += lines[i].len() + 1;
            head.push(lines[i]);
            i += 1;
        } else {
            j -= 1;
            used += lines[j].len() + 1;
            tail.push(lines[j]);
        }
    }
    tail.reverse();
    format!("{}\n[... {} lines omitted ...]\n{}", head.join("\n"), j.saturating_sub(i), tail.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_parse_with_names_and_placeholders() {
        let t = builtin_templates();
        assert_eq!(t.len(), 6);
        let g = t.iter().find(|t| t.id == "general").unwrap();
        assert_eq!(g.name, "General meeting");
        assert!(g.description.starts_with("Balanced"));
        assert!(g.body.contains("{{transcript}}"));
        assert!(!g.body.starts_with('#'));
    }

    #[test]
    fn render_replaces_known_placeholders() {
        let vars = TemplateVars { title: "T".into(), transcript: "hello".into(), ..Default::default() };
        assert_eq!(render("{{title}}: {{transcript}} {{nope}}", &vars), "T: hello {{nope}}");
    }

    #[test]
    fn stamps_roundtrip() {
        assert_eq!(fmt_stamp(65_000), "1:05");
        assert_eq!(fmt_stamp(3_725_000), "1:02:05");
        assert_eq!(parse_stamp("1:05"), Some(65_000));
        assert_eq!(parse_stamp("1:02:05"), Some(3_725_000));
        assert_eq!(parse_stamp("x"), None);
    }

    #[test]
    fn parses_messy_sections() {
        let md = "Here are the notes:\n## Summary\nWe met.\n\n## Decisions\n- Ship Friday\n- None\n\n## Action items\n- [ ] Write docs - Sam (Monday)\n* Call client - Priya\n\n## Open questions\nNone.\n\n## Key moments\n- [1:05] Sam proposed shipping\n- [12:34]: Budget agreed\n- no stamp here\n## Custom\nstuff";
        let s = parse_sections(md);
        assert_eq!(s.summary, "We met.");
        assert_eq!(s.decisions, vec!["Ship Friday"]);
        assert_eq!(s.action_items, vec!["Write docs - Sam (Monday)", "Call client - Priya"]);
        assert!(s.open_questions.is_empty());
        assert_eq!(s.key_moments.len(), 3);
        assert_eq!(s.key_moments[0], KeyMoment { ms: Some(65_000), text: "Sam proposed shipping".into() });
        assert_eq!(s.key_moments[1].ms, Some(754_000));
        assert_eq!(s.key_moments[2].ms, None);
        assert_eq!(s.sections.get("custom").unwrap(), "stuff");
    }

    #[test]
    fn truncate_keeps_head_and_tail() {
        let text: String = (0..100).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let t = truncate_middle(&text, 200);
        assert!(t.starts_with("line 0"));
        assert!(t.ends_with("line 99"));
        assert!(t.contains("lines omitted"));
        assert!(t.len() < 300);
    }
}
