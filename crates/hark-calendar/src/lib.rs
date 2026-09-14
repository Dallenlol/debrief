//! Just enough iCalendar to know what meeting you are probably in: VEVENTs with
//! start/end (UTC, TZID or all-day), title, attendees and a URL. Weekly
//! recurrences are expanded a few weeks ahead; other rules are ignored.
//
// fable: no RRULE beyond simple WEEKLY (no UNTIL/COUNT/BYDAY lists, no EXDATE);
// a real recurrence engine (e.g. the `rrule` crate) is the upgrade path.

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalEvent {
    pub uid: String,
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    /// Display names (CN=) of attendees and the organizer, deduplicated.
    pub attendees: Vec<String>,
    pub url: Option<String>,
    pub location: Option<String>,
}

/// Unfold continuation lines (RFC 5545 §3.1) and split into `NAME;PARAMS:VALUE` lines.
fn unfold(src: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in src.lines() {
        let line = raw.trim_end_matches('\r');
        if (line.starts_with(' ') || line.starts_with('\t')) && !out.is_empty() {
            out.last_mut().unwrap().push_str(&line[1..]);
        } else {
            out.push(line.to_string());
        }
    }
    out
}

type Params = Vec<(String, String)>;

/// `(name, params, value)`; params as `(key, value)` pairs, keys upper-cased.
fn split_line(line: &str) -> Option<(String, Params, String)> {
    let colon = find_value_colon(line)?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let mut parts = head.split(';');
    let name = parts.next()?.trim().to_uppercase();
    let params = parts
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.trim().to_uppercase(), v.trim().trim_matches('"').to_string()))
        .collect();
    Some((name, params, value.to_string()))
}

/// The ':' that ends the property head, skipping any inside quoted parameter values.
fn find_value_colon(line: &str) -> Option<usize> {
    let mut quoted = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ':' if !quoted => return Some(i),
            _ => {}
        }
    }
    None
}

fn unescape(v: &str) -> String {
    v.replace("\\n", "\n").replace("\\N", "\n").replace("\\,", ",").replace("\\;", ";").replace("\\\\", "\\")
}

/// Parse a DTSTART/DTEND value with its parameters into UTC. All-day dates
/// become local midnight in the given zone (or UTC).
fn parse_dt(value: &str, params: &[(String, String)]) -> Option<DateTime<Utc>> {
    let v = value.trim();
    let tzid = params.iter().find(|(k, _)| k == "TZID").map(|(_, v)| v.as_str());
    let is_date = params.iter().any(|(k, v)| k == "VALUE" && v.eq_ignore_ascii_case("DATE")) || (v.len() == 8 && !v.contains('T'));
    if is_date {
        let d = NaiveDate::parse_from_str(&v[..8], "%Y%m%d").ok()?;
        let naive = d.and_hms_opt(0, 0, 0)?;
        return Some(localize(naive, tzid));
    }
    if let Some(stripped) = v.strip_suffix('Z') {
        let naive = NaiveDateTime::parse_from_str(stripped, "%Y%m%dT%H%M%S").ok()?;
        return Some(Utc.from_utc_datetime(&naive));
    }
    let naive = NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S").ok()?;
    Some(localize(naive, tzid))
}

fn localize(naive: NaiveDateTime, tzid: Option<&str>) -> DateTime<Utc> {
    match tzid.and_then(|t| t.parse::<chrono_tz::Tz>().ok().or_else(|| windows_tz(t))) {
        Some(tz) => tz.from_local_datetime(&naive).single().or_else(|| tz.from_local_datetime(&naive).earliest()).map(|d| d.with_timezone(&Utc)).unwrap_or_else(|| Utc.from_utc_datetime(&naive)),
        None => Utc.from_utc_datetime(&naive),
    }
}

/// Outlook exports Windows zone names; map the common ones.
fn windows_tz(name: &str) -> Option<chrono_tz::Tz> {
    let iana = match name {
        "Pacific Standard Time" => "America/Los_Angeles",
        "Mountain Standard Time" => "America/Denver",
        "Central Standard Time" => "America/Chicago",
        "Eastern Standard Time" => "America/New_York",
        "Atlantic Standard Time" => "America/Halifax",
        "GMT Standard Time" => "Europe/London",
        "W. Europe Standard Time" | "Romance Standard Time" | "Central Europe Standard Time" => "Europe/Paris",
        "E. Europe Standard Time" | "FLE Standard Time" => "Europe/Kiev",
        "India Standard Time" => "Asia/Kolkata",
        "China Standard Time" | "Singapore Standard Time" => "Asia/Shanghai",
        "Tokyo Standard Time" => "Asia/Tokyo",
        "AUS Eastern Standard Time" => "Australia/Sydney",
        "New Zealand Standard Time" => "Pacific/Auckland",
        "UTC" | "Coordinated Universal Time" => "UTC",
        _ => return None,
    };
    iana.parse().ok()
}

/// Weekly repeats to expand ahead of the first occurrence.
const WEEKLY_EXPAND: i64 = 8;

/// Parse every VEVENT in an .ics document. Malformed events are skipped.
pub fn parse_ics(src: &str) -> Vec<CalEvent> {
    let mut events = Vec::new();
    let mut cur: Option<Draft> = None;
    for line in unfold(src) {
        if line.eq_ignore_ascii_case("BEGIN:VEVENT") {
            cur = Some(Draft::default());
            continue;
        }
        if line.eq_ignore_ascii_case("END:VEVENT") {
            if let Some(d) = cur.take() {
                events.extend(d.finish());
            }
            continue;
        }
        let Some(d) = cur.as_mut() else { continue };
        let Some((name, params, value)) = split_line(&line) else { continue };
        match name.as_str() {
            "UID" => d.uid = Some(value.trim().to_string()),
            "SUMMARY" => d.title = Some(unescape(value.trim())),
            "DTSTART" => d.start = parse_dt(&value, &params),
            "DTEND" => d.end = parse_dt(&value, &params),
            "DURATION" => d.duration = parse_duration(value.trim()),
            "URL" => d.url = Some(value.trim().to_string()),
            "LOCATION" => d.location = Some(unescape(value.trim())).filter(|s| !s.is_empty()),
            "ATTENDEE" | "ORGANIZER" => {
                let cn = params.iter().find(|(k, _)| k == "CN").map(|(_, v)| v.trim().to_string());
                let name = cn.filter(|c| !c.is_empty() && !c.contains('@')).or_else(|| {
                    // No display name: use the mailbox local part only if it looks like a name.
                    let addr = value.trim().trim_start_matches("mailto:").trim_start_matches("MAILTO:");
                    addr.split('@').next().filter(|l| l.contains('.') || l.contains('_')).map(|l| {
                        l.split(['.', '_']).filter(|p| !p.is_empty()).map(capitalize).collect::<Vec<_>>().join(" ")
                    })
                });
                if let Some(n) = name {
                    if !d.attendees.iter().any(|a| a.eq_ignore_ascii_case(&n)) {
                        d.attendees.push(n);
                    }
                }
            }
            "RRULE" => d.rrule = Some(value.trim().to_uppercase()),
            "STATUS" => d.cancelled = value.trim().eq_ignore_ascii_case("CANCELLED"),
            _ => {}
        }
    }
    events.sort_by_key(|e| e.start);
    events
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// `PT1H30M`, `P1D`, `PT45M` ...
fn parse_duration(v: &str) -> Option<Duration> {
    let v = v.trim_start_matches('+');
    let (sign, v) = match v.strip_prefix('-') {
        Some(r) => (-1, r),
        None => (1, v),
    };
    let v = v.strip_prefix('P')?;
    let mut total = Duration::zero();
    let mut num = String::new();
    let mut in_time = false;
    for c in v.chars() {
        match c {
            'T' => in_time = true,
            d if d.is_ascii_digit() => num.push(d),
            unit => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                total += match (unit, in_time) {
                    ('W', _) => Duration::weeks(n),
                    ('D', _) => Duration::days(n),
                    ('H', true) => Duration::hours(n),
                    ('M', true) => Duration::minutes(n),
                    ('S', true) => Duration::seconds(n),
                    _ => return None,
                };
            }
        }
    }
    Some(total * sign)
}

#[derive(Default)]
struct Draft {
    uid: Option<String>,
    title: Option<String>,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    duration: Option<Duration>,
    attendees: Vec<String>,
    url: Option<String>,
    location: Option<String>,
    rrule: Option<String>,
    cancelled: bool,
}

impl Draft {
    fn finish(self) -> Vec<CalEvent> {
        if self.cancelled {
            return Vec::new();
        }
        let Some(start) = self.start else { return Vec::new() };
        let end = self.end.or_else(|| self.duration.map(|d| start + d)).unwrap_or(start + Duration::hours(1));
        if end < start {
            return Vec::new();
        }
        let uid = self.uid.unwrap_or_else(|| format!("{}-{}", start.timestamp(), self.title.clone().unwrap_or_default()));
        let base = CalEvent {
            uid,
            title: self.title.unwrap_or_else(|| "Untitled event".into()),
            start,
            end,
            attendees: self.attendees,
            url: self.url,
            location: self.location,
        };
        let weekly = self
            .rrule
            .as_deref()
            .filter(|r| r.contains("FREQ=WEEKLY") && !r.contains("UNTIL=") && !r.contains("COUNT="))
            .map(|r| r.split(';').find_map(|p| p.strip_prefix("INTERVAL=")).and_then(|i| i.parse::<i64>().ok()).unwrap_or(1));
        match weekly {
            Some(interval) => (0..=WEEKLY_EXPAND)
                .map(|i| {
                    let shift = Duration::weeks(i * interval);
                    CalEvent { start: base.start + shift, end: base.end + shift, ..base.clone() }
                })
                .collect(),
            None => vec![base],
        }
    }
}

/// Events that started up to `before` ago or start within `after` of `now`, soonest first.
pub fn events_around(events: &[CalEvent], now: DateTime<Utc>, before: Duration, after: Duration) -> Vec<CalEvent> {
    let mut v: Vec<CalEvent> = events.iter().filter(|e| e.end >= now - before && e.start <= now + after).cloned().collect();
    v.sort_by_key(|e| e.start);
    v
}

/// The event you are most likely in right now: one that overlaps `now`
/// (with 10 minutes of slack either side), preferring the latest start.
pub fn best_match(events: &[CalEvent], now: DateTime<Utc>) -> Option<&CalEvent> {
    let slack = Duration::minutes(10);
    events.iter().filter(|e| e.start - slack <= now && now <= e.end + slack).max_by_key(|e| e.start)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOGLE: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nDTSTART:20260914T170000Z\r\nDTEND:20260914T173000Z\r\nUID:abc123@google.com\r\nORGANIZER;CN=Sarah Chen:mailto:sarah@example.com\r\nATTENDEE;CUTYPE=INDIVIDUAL;ROLE=REQ-PARTICIPANT;PARTSTAT=ACCEPTED;CN=Marcus\r\n  Webb;X-NUM-GUESTS=0:mailto:marcus@example.com\r\nATTENDEE;CN=sarah@example.com:mailto:sarah@example.com\r\nATTENDEE:mailto:priya.patel@example.com\r\nSUMMARY:Q4 launch planning\\, part 2\r\nURL:https://meet.google.com/abc-defg-hij\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nDTSTART;TZID=America/Vancouver:20260915T090000\r\nDTEND;TZID=America/Vancouver:20260915T091500\r\nRRULE:FREQ=WEEKLY;BYDAY=TU\r\nUID:standup\r\nSUMMARY:Standup\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nDTSTART;VALUE=DATE:20260916\r\nDTEND;VALUE=DATE:20260917\r\nUID:allday\r\nSUMMARY:Offsite\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nDTSTART:20260914T180000Z\r\nDURATION:PT45M\r\nUID:cancelled\r\nSTATUS:CANCELLED\r\nSUMMARY:Nope\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn parses_google_style_feed() {
        let ev = parse_ics(GOOGLE);
        assert_eq!(ev.len(), 1 + (WEEKLY_EXPAND as usize + 1) + 1);
        let q4 = ev.iter().find(|e| e.uid == "abc123@google.com").unwrap();
        assert_eq!(q4.title, "Q4 launch planning, part 2");
        assert_eq!(q4.attendees, vec!["Sarah Chen", "Marcus Webb", "Priya Patel"]);
        assert_eq!(q4.url.as_deref(), Some("https://meet.google.com/abc-defg-hij"));
        assert_eq!(q4.end - q4.start, Duration::minutes(30));
        let standups: Vec<&CalEvent> = ev.iter().filter(|e| e.uid == "standup").collect();
        assert_eq!(standups.len(), WEEKLY_EXPAND as usize + 1);
        // 09:00 Vancouver (PDT, UTC-7) = 16:00Z
        assert_eq!(standups[0].start, Utc.with_ymd_and_hms(2026, 9, 15, 16, 0, 0).unwrap());
        assert_eq!(standups[1].start - standups[0].start, Duration::weeks(1));
        let offsite = ev.iter().find(|e| e.uid == "allday").unwrap();
        assert_eq!(offsite.end - offsite.start, Duration::days(1));
        assert!(ev.iter().all(|e| e.uid != "cancelled"));
    }

    #[test]
    fn outlook_windows_zone_and_duration() {
        let src = "BEGIN:VEVENT\nDTSTART;TZID=\"Pacific Standard Time\":20260120T140000\nDURATION:PT1H30M\nUID:o1\nSUMMARY:Review\nEND:VEVENT\n";
        let ev = parse_ics(src);
        assert_eq!(ev.len(), 1);
        // 14:00 PST (UTC-8 in January) = 22:00Z
        assert_eq!(ev[0].start, Utc.with_ymd_and_hms(2026, 1, 20, 22, 0, 0).unwrap());
        assert_eq!(ev[0].end - ev[0].start, Duration::minutes(90));
        assert_eq!(parse_duration("P1DT2H"), Some(Duration::hours(26)));
        assert_eq!(parse_duration("garbage"), None);
    }

    #[test]
    fn matching_windows() {
        let ev = parse_ics(GOOGLE);
        let now = Utc.with_ymd_and_hms(2026, 9, 14, 16, 55, 0).unwrap(); // 5 min before Q4
        assert_eq!(best_match(&ev, now).map(|e| e.uid.as_str()), Some("abc123@google.com"));
        let later = Utc.with_ymd_and_hms(2026, 9, 14, 17, 45, 0).unwrap(); // 15 min after end
        assert_eq!(best_match(&ev, later), None);
        let around = events_around(&ev, now, Duration::minutes(10), Duration::hours(24));
        assert_eq!(around.first().map(|e| e.uid.as_str()), Some("abc123@google.com"));
        assert!(around.iter().any(|e| e.uid == "standup"));
        assert!(around.len() < ev.len());
    }
}
