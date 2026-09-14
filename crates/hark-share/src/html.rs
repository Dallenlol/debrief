//! Standalone HTML page for a meeting (also served by the LAN server).

use hark_store::{Meeting, Segment, Summary};
use html_escape::encode_text as esc;

pub struct PageData<'a> {
    pub meeting: &'a Meeting,
    pub segments: &'a [Segment],
    pub summary: Option<&'a Summary>,
    /// URL of the media file relative to the page (or absolute).
    pub media_url: Option<&'a str>,
    pub is_video: bool,
    /// Clip range: only segments inside it are shown and the player is offset.
    pub clip: Option<(i64, i64)>,
}

fn stamp(ms: i64) -> String {
    let t = (ms.max(0) / 1000) as u64;
    let (h, m, s) = (t / 3600, (t % 3600) / 60, t % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// Minimal markdown -> HTML for the summary (headings, bullets, paragraphs, bold).
fn md_to_html(md: &str) -> String {
    let mut out = String::new();
    let mut in_list = false;
    for line in md.lines() {
        let t = line.trim();
        if let Some(h) = t.strip_prefix("## ").or_else(|| t.strip_prefix("# ")) {
            if in_list { out.push_str("</ul>"); in_list = false; }
            out.push_str(&format!("<h3>{}</h3>", esc(h.trim_end_matches(':'))));
        } else if let Some(li) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            if !in_list { out.push_str("<ul>"); in_list = true; }
            let li = li.trim_start_matches("[ ] ").trim_start_matches("[x] ");
            out.push_str(&format!("<li>{}</li>", inline(li)));
        } else if t.is_empty() {
            if in_list { out.push_str("</ul>"); in_list = false; }
        } else {
            if in_list { out.push_str("</ul>"); in_list = false; }
            out.push_str(&format!("<p>{}</p>", inline(t)));
        }
    }
    if in_list { out.push_str("</ul>"); }
    out
}

fn inline(s: &str) -> String {
    let e = esc(s).to_string();
    // **bold** and [m:ss] seek links
    let mut out = String::new();
    let mut rest = e.as_str();
    while let Some(i) = rest.find("**") {
        out.push_str(&rest[..i]);
        if let Some(j) = rest[i + 2..].find("**") {
            out.push_str(&format!("<b>{}</b>", &rest[i + 2..i + 2 + j]));
            rest = &rest[i + 4 + j..];
        } else {
            out.push_str("**");
            rest = &rest[i + 2..];
        }
    }
    out.push_str(rest);
    // timestamps
    let mut res = String::new();
    let mut r = out.as_str();
    while let Some(i) = r.find('[') {
        res.push_str(&r[..i]);
        if let Some(j) = r[i..].find(']') {
            let inner = &r[i + 1..i + j];
            let head = inner.split('@').next().unwrap_or("").trim();
            if let Some(ms) = parse_stamp(head) {
                res.push_str(&format!("<a href=\"#\" class=\"t\" data-ms=\"{ms}\">{}</a>", esc(head)));
            } else {
                res.push_str(&format!("[{inner}]"));
            }
            r = &r[i + j + 1..];
        } else {
            res.push_str(&r[i..]);
            r = "";
        }
    }
    res.push_str(r);
    res
}

fn parse_stamp(s: &str) -> Option<i64> {
    let parts: Vec<u64> = s.split(':').map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
    let secs = match parts.as_slice() { [m, s] => m * 60 + s, [h, m, s] => h * 3600 + m * 60 + s, _ => return None };
    Some((secs * 1000) as i64)
}

pub fn render_standalone(d: &PageData) -> String {
    let (clip_start, clip_end) = d.clip.unwrap_or((0, i64::MAX));
    let mut transcript = String::new();
    let mut last: Option<&str> = None;
    for s in d.segments.iter().filter(|s| s.end_ms >= clip_start && s.start_ms <= clip_end) {
        let sp = s.speaker.as_deref();
        if sp != last {
            if let Some(n) = sp {
                transcript.push_str(&format!("<div class=\"sp\">{}</div>", esc(n)));
            }
            last = sp;
        }
        let text = s.clean_text.as_deref().unwrap_or(&s.text);
        transcript.push_str(&format!(
            "<div class=\"seg\" data-ms=\"{}\"><span class=\"ts\">{}</span><span>{}</span></div>",
            s.start_ms - clip_start.max(0),
            stamp(s.start_ms - clip_start.max(0)),
            esc(text)
        ));
    }
    let summary = d.summary.map(|s| md_to_html(&s.markdown)).unwrap_or_default();
    let media = match d.media_url {
        Some(url) if d.is_video => format!("<video id=\"m\" controls src=\"{}\"></video>", esc(url)),
        Some(url) => format!("<audio id=\"m\" controls src=\"{}\"></audio>", esc(url)),
        None => String::new(),
    };
    let title = esc(&d.meeting.title);
    let date = d.meeting.started_at.format("%b %-d, %Y").to_string();
    format!(
        r##"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title} - Hark</title>
<style>
:root{{color-scheme:light dark;--ink:#221f1c;--ink2:#6b655f;--line:#e8e3dc;--accent:#d9542d;--bg:#fbf9f5}}
@media(prefers-color-scheme:dark){{:root{{--ink:#f1ede7;--ink2:#a49c93;--line:#3a3531;--bg:#1b1917}}}}
body{{margin:0;font:15px/1.55 -apple-system,Segoe UI,Roboto,sans-serif;color:var(--ink);background:var(--bg)}}
main{{max-width:880px;margin:0 auto;padding:32px 20px 80px}}
h1{{font-family:Georgia,serif;font-weight:400;font-size:34px;margin:0 0 4px}}
.meta{{color:var(--ink2);font-size:13px;margin-bottom:20px}}
video,audio{{width:100%;border-radius:12px;background:#000;margin-bottom:20px}}audio{{background:transparent}}
h2{{font-size:11px;letter-spacing:.12em;text-transform:uppercase;color:var(--ink2);margin:28px 0 8px}}
h3{{font-size:14px;margin:16px 0 4px}}ul{{margin:4px 0;padding-left:20px}}
.sp{{font-size:12px;font-weight:600;color:var(--accent);margin:14px 0 2px}}
.seg{{display:flex;gap:12px;padding:3px 6px;border-radius:6px;cursor:pointer}}.seg:hover{{background:rgba(127,127,127,.08)}}
.seg.on{{background:rgba(217,84,45,.12)}}.ts{{font-family:ui-monospace,monospace;font-size:11px;color:var(--ink2);min-width:44px;padding-top:3px}}
a.t{{color:var(--accent);text-decoration:none;font-family:ui-monospace,monospace;font-size:12px;background:rgba(217,84,45,.1);padding:0 4px;border-radius:4px}}
footer{{margin-top:48px;color:var(--ink2);font-size:12px}}
</style></head><body><main>
<h1>{title}</h1><div class="meta">{date} &middot; {duration}{clip_note}</div>
{media}
{summary_block}
<h2>Transcript</h2><div id="tr">{transcript}</div>
<footer>Shared from Hark, a local-only meeting recorder. Nothing here touched the cloud.</footer>
</main>
<script>
const m=document.getElementById('m');
function seek(ms){{if(!m)return;m.currentTime=ms/1000;m.play();}}
document.querySelectorAll('.seg').forEach(e=>e.addEventListener('click',()=>seek(+e.dataset.ms)));
document.querySelectorAll('a.t').forEach(e=>e.addEventListener('click',ev=>{{ev.preventDefault();seek(+e.dataset.ms)}}));
if(m){{m.addEventListener('timeupdate',()=>{{const t=m.currentTime*1000;let cur=null;document.querySelectorAll('.seg').forEach(e=>{{if(+e.dataset.ms<=t)cur=e;e.classList.remove('on')}});if(cur)cur.classList.add('on');}});}}
</script></body></html>"##,
        title = title,
        date = esc(&date),
        duration = stamp(d.clip.map(|(a, b)| b - a).unwrap_or(d.meeting.duration_ms)),
        clip_note = d.clip.map(|(a, b)| format!(" &middot; clip {}&ndash;{}", stamp(a), stamp(b))).unwrap_or_default(),
        media = media,
        summary_block = if summary.is_empty() { String::new() } else { format!("<h2>Summary</h2>{summary}") },
        transcript = transcript,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_escaped_page_with_clip_offsets() {
        let mut m = Meeting::new_recording("Q&A <script>", None);
        m.duration_ms = 90_000;
        let segs = vec![
            Segment { id: 1, meeting_id: m.id.clone(), start_ms: 0, end_ms: 5_000, speaker: Some("Sam".into()), text: "before".into(), clean_text: None },
            Segment { id: 2, meeting_id: m.id.clone(), start_ms: 30_000, end_ms: 35_000, speaker: Some("Sam".into()), text: "<b>inside</b>".into(), clean_text: Some("inside clean".into()) },
        ];
        let html = render_standalone(&PageData { meeting: &m, segments: &segs, summary: None, media_url: Some("clip.mp3"), is_video: false, clip: Some((30_000, 60_000)) });
        assert!(html.contains("Q&amp;A &lt;script&gt;"));
        assert!(!html.contains("before"));
        assert!(html.contains("inside clean"));
        assert!(html.contains("data-ms=\"0\""), "clip segments are re-based to 0");
        assert!(html.contains("<audio"));
    }

    #[test]
    fn markdown_summary_with_timestamps() {
        let h = md_to_html("## Key moments\n- [1:05] **Sam** agreed\n\nDone.");
        assert!(h.contains("<h3>Key moments</h3>"));
        assert!(h.contains("data-ms=\"65000\""));
        assert!(h.contains("<b>Sam</b>"));
        assert!(h.contains("<p>Done.</p>"));
    }
}
