//! The weekly report (FR-SCH-007…010, BR-10; M5 part 3): a note in `reports/weekly/<YYYY>-W<ww>.md`
//! with the week's highlights, written by the model from the daily summaries (FR-SCH-008), and the
//! numbers Rust counts: tasks, measurements against the week before, next week's dated tasks.

use chrono::{DateTime, Datelike, Duration, FixedOffset, IsoWeek, NaiveDate, NaiveTime, Weekday};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::extraction::MetricKind;
use crate::vault::{Folders, Vault};

/// When the report is written (FR-SCH-016). The default is Sunday 20:00.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WeeklySchedule {
    pub enabled: bool,
    /// 0 = Monday … 6 = Sunday.
    pub weekday: u8,
    /// "HH:MM".
    pub time: String,
}

impl Default for WeeklySchedule {
    fn default() -> Self {
        Self { enabled: true, weekday: 6, time: "20:00".into() }
    }
}

/// An ISO week (Monday to Sunday, BR-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Week {
    pub year: i32,
    pub number: u32,
}

impl Week {
    pub fn of(date: NaiveDate) -> Self {
        let w: IsoWeek = date.iso_week();
        Self { year: w.year(), number: w.week() }
    }

    pub fn monday(self) -> NaiveDate {
        NaiveDate::from_isoywd_opt(self.year, self.number, Weekday::Mon).expect("a real ISO week")
    }

    pub fn sunday(self) -> NaiveDate {
        self.monday() + Duration::days(6)
    }

    pub fn previous(self) -> Self {
        Self::of(self.monday() - Duration::days(7))
    }

    pub fn next(self) -> Self {
        Self::of(self.monday() + Duration::days(7))
    }

    /// "2026-W41", as the file is named (FR-SCH-009).
    pub fn label(self) -> String {
        format!("{}-W{:02}", self.year, self.number)
    }

    pub fn parse(label: &str) -> Option<Self> {
        let (y, w) = label.split_once("-W")?;
        if w.len() != 2 {
            return None;
        }
        let week = Self { year: y.parse().ok()?, number: w.parse().ok()? };
        NaiveDate::from_isoywd_opt(week.year, week.number, Weekday::Mon).map(|_| week)
    }
}

/// When `week`'s report is due, in the offset of `now`'s clock (final review I6). On Sunday the
/// report is the week's own, written on its last evening; any other day writes it in the next week,
/// once the week is over, so no day of a week is left out of every report (final review I5).
pub fn slot(week: Week, schedule: &WeeklySchedule, offset: FixedOffset) -> DateTime<FixedOffset> {
    let day = i64::from(schedule.weekday.min(6));
    let date = if day == 6 { week.sunday() } else { week.next().monday() + Duration::days(day) };
    let time = NaiveTime::parse_from_str(&schedule.time, "%H:%M").unwrap_or(NaiveTime::from_hms_opt(20, 0, 0).expect("a real time"));
    date.and_time(time).and_local_timezone(offset).single().expect("a fixed offset has every time")
}

/// The week whose report is due at `now`: the latest one whose slot has come (FR-SCH-007). Only that
/// one: weeks missed before it are not caught up (FR-SCH-012).
pub fn due_week(now: DateTime<FixedOffset>, schedule: &WeeklySchedule) -> Option<Week> {
    if !schedule.enabled {
        return None;
    }
    let this = Week::of(now.date_naive());
    [this, this.previous(), this.previous().previous()].into_iter().find(|w| slot(*w, schedule, *now.offset()) <= now)
}

/// `reports/weekly/2026-W41.md` (FR-SCH-009).
pub fn report_rel(folders: &Folders, week: Week) -> String {
    format!("{}/weekly/{}.md", folders.reports.trim_end_matches('/'), week.label())
}

/// One measurement kind's week next to the week before.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricLine {
    pub kind: MetricKind,
    pub this_week: Option<f64>,
    pub last_week: Option<f64>,
}

/// What Rust counts for the report (FR-SCH-008): never the model.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct WeekStats {
    pub completed: Vec<String>,
    pub added: usize,
    /// Open tasks dated in the week (still open when the report is written).
    pub open_late: Vec<String>,
    /// (date, time, title) of next week's open tasks.
    pub next_week: Vec<(String, Option<String>, String)>,
    pub metrics: Vec<MetricLine>,
}

fn day_range(week: Week) -> (String, String) {
    (week.monday().to_string(), week.sunday().to_string())
}

/// A week of one kind: average per day with a value; the workout's sessions summed.
fn week_value(conn: &Connection, kind: MetricKind, week: Week) -> rusqlite::Result<Option<f64>> {
    let days = crate::metrics::daily_values(conn, kind, week.monday(), week.sunday()).map_err(|e| match e {
        crate::metrics::MetricError::Db(e) => e,
        other => rusqlite::Error::InvalidParameterName(other.to_string()),
    })?;
    if days.is_empty() {
        return Ok(None);
    }
    let sum: f64 = days.iter().map(|d| d.value).sum();
    Ok(Some(match kind {
        MetricKind::Workout => sum,
        MetricKind::Weight => days.last().map_or(sum, |d| d.value), // where the week ended
        _ => sum / days.len() as f64,
    }))
}

pub fn week_stats(conn: &Connection, week: Week) -> rusqlite::Result<WeekStats> {
    let (from, to) = day_range(week);
    let titles = |sql: &str, a: &str, b: &str| -> rusqlite::Result<Vec<String>> { conn.prepare(sql)?.query_map([a, b], |r| r.get(0))?.collect() };
    let completed = titles("SELECT title FROM task WHERE status = 'done' AND substr(completed_at, 1, 10) BETWEEN ?1 AND ?2 ORDER BY completed_at", &from, &to)?;
    let added: i64 = conn.query_row("SELECT count(*) FROM task WHERE substr(created_at, 1, 10) BETWEEN ?1 AND ?2", [&from, &to], |r| r.get(0))?;
    let open_late = conn
        .prepare("SELECT title FROM task WHERE status = 'open' AND date BETWEEN ?1 AND ?2 ORDER BY date, time")?
        .query_map([&from, &to], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let (n_from, n_to) = day_range(week.next());
    let next_week = conn
        .prepare("SELECT date, time, title FROM task WHERE status = 'open' AND date BETWEEN ?1 AND ?2 ORDER BY date, time IS NULL, time")?
        .query_map([&n_from, &n_to], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    let mut metrics = Vec::new();
    for kind in [MetricKind::Sleep, MetricKind::Steps, MetricKind::Water, MetricKind::Weight, MetricKind::Workout] {
        let (this_week, last_week) = (week_value(conn, kind, week)?, week_value(conn, kind, week.previous())?);
        if this_week.is_some() || last_week.is_some() {
            metrics.push(MetricLine { kind, this_week, last_week });
        }
    }
    Ok(WeekStats { completed, added: added as usize, open_late, next_week, metrics })
}

/// The week's daily summaries, oldest first (FR-SCH-008: the model reads these, not the notes).
pub fn week_summaries(cache: &Connection, week: Week) -> rusqlite::Result<Vec<(NaiveDate, String)>> {
    let (from, to) = day_range(week);
    cache
        .prepare("SELECT date, summary_text FROM daily_summary WHERE date BETWEEN ?1 AND ?2 ORDER BY date")?
        .query_map([&from, &to], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .filter_map(|r| match r {
            Ok((d, t)) => NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok().map(|d| Ok((d, t))),
            Err(e) => Some(Err(e)),
        })
        .collect()
}

const DAY_EN: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

/// The request for the week's highlights, from the daily summaries.
pub fn highlights_request(week: Week, summaries: &[(NaiveDate, String)], lang: &str) -> Value {
    let mut text = format!("Week {} ({} to {})\n\n", week.label(), week.monday(), week.sunday());
    for (date, summary) in summaries {
        text.push_str(&format!("{date} {}:\n{}\n\n", DAY_EN[date.weekday().num_days_from_monday() as usize], summary.trim()));
    }
    let language = if lang == "en" { "English" } else { "Turkish" };
    json!({
        "messages": [
            { "role": "system", "content": format!("You write the highlights of the user's week from their daily summaries. Write 3 to 5 short bullet points, each starting with \"- \", in {language}. Use only what the summaries say: never add, guess or judge. Keep the [[Note title]] links they name. No heading, no introduction.") },
            { "role": "user", "content": text }
        ],
        "temperature": 0.2,
        "max_tokens": 400,
        "cache_prompt": false,
        "chat_template_kwargs": { "enable_thinking": false }
    })
}

/// The model's bullets, kept to the lines that are bullets.
pub fn clean_highlights(raw: &str) -> Option<String> {
    let lines: Vec<String> = raw
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("- ") || l.starts_with("* ") || l.starts_with("• "))
        .map(|l| format!("- {}", l[l.char_indices().nth(2).map_or(l.len(), |(i, _)| i)..].trim()))
        .take(5)
        .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// Why the highlights are missing, when they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoHighlights {
    /// The week has no daily summaries to read.
    NoSummaries,
    /// No model is installed: the numbers still come.
    NoModel,
    /// The model failed twice or wrote no points (final review I2): the numbers still come.
    Failed,
}

const MONTHS_TR: [&str; 12] = ["Ocak", "Şubat", "Mart", "Nisan", "Mayıs", "Haziran", "Temmuz", "Ağustos", "Eylül", "Ekim", "Kasım", "Aralık"];
const MONTHS_EN: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const DAY_SHORT_TR: [&str; 7] = ["Pzt", "Sal", "Çar", "Per", "Cum", "Cmt", "Paz"];
const DAY_SHORT_EN: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// "5–11 Ekim 2026", "29 Eylül – 5 Ekim 2026", "29 December 2025 – 4 January 2026".
pub fn span(week: Week, lang: &str) -> String {
    let months = if lang == "en" { MONTHS_EN } else { MONTHS_TR };
    let (a, b) = (week.monday(), week.sunday());
    let m = |d: NaiveDate| months[d.month0() as usize];
    if a.year() != b.year() {
        format!("{} {} {} – {} {} {}", a.day(), m(a), a.year(), b.day(), m(b), b.year())
    } else if a.month() != b.month() {
        format!("{} {} – {} {} {}", a.day(), m(a), b.day(), m(b), b.year())
    } else {
        format!("{}–{} {} {}", a.day(), b.day(), m(b), b.year())
    }
}

/// A number as the language writes it: "6,8" in Turkish, "6.8" in English; thousands grouped.
fn number(v: f64, decimals: usize, lang: &str) -> String {
    let s = format!("{v:.decimals$}");
    let (int, frac) = s.split_once('.').map_or((s.as_str(), None), |(i, f)| (i, Some(f)));
    let (sign, digits) = int.strip_prefix('-').map_or(("", int), |d| ("-", d));
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(if lang == "en" { ',' } else { '.' });
        }
        grouped.push(c);
    }
    match frac {
        Some(f) if f.trim_end_matches('0').is_empty() => format!("{sign}{grouped}"),
        Some(f) => format!("{sign}{grouped}{}{f}", if lang == "en" { '.' } else { ',' }),
        None => format!("{sign}{grouped}"),
    }
}

fn metric_cell(kind: MetricKind, v: Option<f64>, lang: &str) -> String {
    let Some(v) = v else { return "–".into() };
    let tr = lang != "en";
    match kind {
        MetricKind::Sleep => format!("{} {}", number(v, 1, lang), if tr { "sa" } else { "h" }),
        MetricKind::Steps => number(v, 0, lang),
        MetricKind::Water => format!("{} L", number(v / 1000.0, 1, lang)),
        MetricKind::Weight => format!("{} kg", number(v, 1, lang)),
        MetricKind::Workout => format!(
            "{} {}",
            number(v, 0, lang),
            if tr {
                "oturum"
            } else {
                if v == 1.0 {
                    "session"
                } else {
                    "sessions"
                }
            }
        ),
    }
}

fn metric_name(kind: MetricKind, lang: &str) -> &'static str {
    match (kind, lang == "en") {
        (MetricKind::Sleep, false) => "Uyku (günlük ort.)",
        (MetricKind::Sleep, true) => "Sleep (daily avg.)",
        (MetricKind::Steps, false) => "Adım (günlük ort.)",
        (MetricKind::Steps, true) => "Steps (daily avg.)",
        (MetricKind::Water, false) => "Su (günlük ort.)",
        (MetricKind::Water, true) => "Water (daily avg.)",
        (MetricKind::Weight, false) => "Kilo (hafta sonu)",
        (MetricKind::Weight, true) => "Weight (end of week)",
        (MetricKind::Workout, false) => "Antrenman",
        (MetricKind::Workout, true) => "Workouts",
    }
}

/// The report's Markdown (FR-SCH-009: `pla_generated`, so it is never mined for tasks).
pub fn render(week: Week, lang: &str, highlights: Result<&str, NoHighlights>, stats: &WeekStats) -> String {
    let en = lang == "en";
    let t = |tr: &'static str, e: &'static str| if en { e } else { tr };
    let mut s = format!("---\npla_generated: true\nweek: {}\n---\n# {} {} · {}\n\n", week.label(), t("Hafta", "Week"), week.number, span(week, lang));
    s.push_str(&format!("## {}\n\n", t("Haftanın özeti", "The week in brief")));
    match highlights {
        Ok(h) => s.push_str(h.trim()),
        Err(NoHighlights::NoSummaries) => s.push_str(t("_Bu hafta için günlük özet yok._", "_No daily summaries this week._")),
        Err(NoHighlights::NoModel) => {
            s.push_str(t("_Dil modeli olmadan özet yazılamadı; sayılar aşağıda._", "_No summary without the language model; the numbers are below._"))
        }
        Err(NoHighlights::Failed) => s.push_str(t(
            "_Dil modeli bu hafta için özet yazamadı; sayılar aşağıda._",
            "_The language model could not write this week's summary; the numbers are below._",
        )),
    }
    s.push_str(&format!("\n\n## {}\n\n", t("Görevler", "Tasks")));
    s.push_str(&format!(
        "{} {} · {} {} · {} {}\n",
        t("Tamamlanan", "Completed"),
        stats.completed.len(),
        t("eklenen", "added"),
        stats.added,
        t("bekleyen", "still open"),
        stats.open_late.len()
    ));
    // at most ten names a list, the rest counted (final review M3)
    let names = |list: &[String]| {
        let shown = list.iter().take(10).cloned().collect::<Vec<_>>().join(", ");
        if list.len() > 10 {
            format!(
                "{shown} {}{}",
                if en { "and " } else { "ve " },
                if en { format!("{} more", list.len() - 10) } else { format!("{} tane daha", list.len() - 10) }
            )
        } else {
            shown
        }
    };
    if !stats.completed.is_empty() {
        s.push_str(&format!("\n{}: {}\n", t("Tamamlananlar", "Done"), names(&stats.completed)));
    }
    if !stats.open_late.is_empty() {
        s.push_str(&format!("\n{}: {}\n", t("Bekleyenler", "Still open"), names(&stats.open_late)));
    }
    s.push_str(&format!("\n## {}\n\n", t("Ölçümler", "Measurements")));
    if stats.metrics.is_empty() {
        s.push_str(t("_Bu iki haftada ölçüm kaydı yok._\n", "_No measurements in these two weeks._\n"));
    } else {
        s.push_str(&format!("| | {} | {} |\n| --- | --- | --- |\n", t("Bu hafta", "This week"), t("Geçen hafta", "Last week")));
        for m in &stats.metrics {
            s.push_str(&format!(
                "| {} | {} | {} |\n",
                metric_name(m.kind, lang),
                metric_cell(m.kind, m.this_week, lang),
                metric_cell(m.kind, m.last_week, lang)
            ));
        }
    }
    s.push_str(&format!("\n## {}\n\n", t("Gelecek hafta", "Next week")));
    if stats.next_week.is_empty() {
        s.push_str(t("_Tarihli açık görev yok._\n", "_No dated open tasks._\n"));
    } else {
        let days = if en { DAY_SHORT_EN } else { DAY_SHORT_TR };
        let months = if en { MONTHS_EN } else { MONTHS_TR };
        for (date, time, title) in &stats.next_week {
            let when = NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .map(|d| format!("{} {} {}", d.day(), &months[d.month0() as usize], days[d.weekday().num_days_from_monday() as usize]))
                .unwrap_or_else(|_| date.clone());
            let at = time.as_deref().map(|t| format!(" {t}")).unwrap_or_default();
            s.push_str(&format!("- {when}{at} · {title}\n"));
        }
    }
    s
}

#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Path(String),
}

/// Writes the report; `Ok(None)` when it is there already (FR-SCH-010: never overwritten). It is
/// written whole beside its place, then linked in, which fails if a report appeared meanwhile: a
/// full disk never leaves a half report that would count as written (final review I1).
pub fn write_report(vault: &Vault, week: Week, text: &str) -> Result<Option<String>, ReportError> {
    let rel = report_rel(&vault.config.folders, week);
    let path = crate::files::resolve(vault, &rel).map_err(|e| ReportError::Path(e.to_string()))?;
    if path.exists() {
        return Ok(None);
    }
    let dir = path.parent().ok_or_else(|| ReportError::Path(rel.clone()))?;
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{}.{}.tmp", week.label(), std::process::id()));
    std::fs::write(&tmp, text).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    let linked = std::fs::hard_link(&tmp, &path);
    let _ = std::fs::remove_file(&tmp);
    match linked {
        Ok(()) => Ok(Some(rel)),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Whether the report of `week` exists in the vault.
pub fn report_exists(vault: &Vault, week: Week) -> bool {
    crate::files::resolve(vault, &report_rel(&vault.config.folders, week)).is_ok_and(|p| p.exists())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::{add_task_as, TaskInput};

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn at(s: &str) -> DateTime<FixedOffset> {
        chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap().and_local_timezone(FixedOffset::east_opt(3 * 3600).unwrap()).unwrap()
    }

    #[test]
    fn weeks_are_iso_weeks_also_at_the_year_end() {
        // BR-10
        assert_eq!(Week::of(d("2026-10-09")).label(), "2026-W41");
        assert_eq!(Week::of(d("2026-10-09")).monday(), d("2026-10-05"));
        assert_eq!(Week::of(d("2026-12-31")).label(), "2026-W53");
        assert_eq!(Week::of(d("2027-01-03")).label(), "2026-W53");
        assert_eq!(Week::of(d("2025-12-29")).label(), "2026-W01");
        assert_eq!(Week::parse("2026-W41"), Some(Week::of(d("2026-10-09"))));
        assert_eq!(Week::parse("2026-W54"), None);
        assert_eq!(Week::parse("2026-W5"), None, "two digits, as the files are named");
    }

    #[test]
    fn the_due_week_is_the_latest_whose_time_has_come() {
        // FR-SCH-007/012: Sunday 20:00 by default; Monday morning still asks for last week's
        let s = WeeklySchedule::default();
        assert_eq!(due_week(at("2026-10-11 19:59"), &s).map(Week::label).as_deref(), Some("2026-W40"));
        assert_eq!(due_week(at("2026-10-11 20:00"), &s).map(Week::label).as_deref(), Some("2026-W41"));
        assert_eq!(due_week(at("2026-10-12 09:00"), &s).map(Week::label).as_deref(), Some("2026-W41"));
        // a Monday 09:00 report covers the week just past
        let monday = WeeklySchedule { weekday: 0, time: "09:00".into(), ..WeeklySchedule::default() };
        assert_eq!(due_week(at("2026-10-12 09:00"), &monday).map(Week::label).as_deref(), Some("2026-W41"));
        assert_eq!(due_week(at("2026-10-12 08:59"), &monday).map(Week::label).as_deref(), Some("2026-W40"));
        // final review I5: a Friday report covers the last whole week, so no day falls between
        let friday = WeeklySchedule { weekday: 4, time: "18:00".into(), ..WeeklySchedule::default() };
        assert_eq!(due_week(at("2026-10-16 18:00"), &friday).map(Week::label).as_deref(), Some("2026-W41"));
        assert_eq!(due_week(at("2026-10-16 17:59"), &friday).map(Week::label).as_deref(), Some("2026-W40"));
        // across the year's end: W53 is reported on the Monday of 2027-W01
        assert_eq!(due_week(at("2027-01-04 09:00"), &monday).map(Week::label).as_deref(), Some("2026-W53"));
        // final review I6: the slot is read in the clock's own offset, not the machine's zone
        let utc = chrono::DateTime::parse_from_rfc3339("2026-10-11T20:00:00+00:00").unwrap();
        assert_eq!(due_week(utc, &s).map(Week::label).as_deref(), Some("2026-W41"));
        assert_eq!(due_week(at("2026-10-12 09:00"), &WeeklySchedule { enabled: false, ..s }), None, "FR-SCH-016");
    }

    fn db() -> (tempfile::TempDir, Connection) {
        let tmp = tempfile::tempdir().unwrap();
        let conn = crate::db::open_databases(tmp.path()).unwrap().pla;
        (tmp, conn)
    }

    #[test]
    fn the_numbers_come_from_the_records() {
        // FR-SCH-008
        let (_t, conn) = db();
        let input = |title: &str, date: &str| TaskInput { title: title.into(), details: None, date: Some(date.into()), time: None, remind: Some(false) };
        let made = at("2026-10-06 09:00");
        let pay = add_task_as(&conn, &input("Fatura öde", "2026-10-07"), "manual", made).unwrap();
        crate::tasks::set_done(&conn, &pay, true, at("2026-10-07 18:00")).unwrap();
        add_task_as(&conn, &input("Rapor yaz", "2026-10-09"), "manual", made).unwrap();
        add_task_as(&conn, &input("Dişçi", "2026-10-13"), "manual", made).unwrap();
        add_task_as(&conn, &input("Eski iş", "2026-10-01"), "manual", at("2026-09-28 09:00")).unwrap();
        let s = crate::extraction::ValidationSettings::default();
        let sleep = |date: &str, h: f64| {
            crate::metrics::log_metric(
                &conn,
                &crate::metrics::MetricInput {
                    kind: MetricKind::Sleep,
                    date: date.into(),
                    value: Some(h),
                    unit: None,
                    exercise: None,
                    sets: None,
                    reps: None,
                    confirmed: false,
                },
                &s,
                at("2026-10-11 21:00"),
            )
            .unwrap()
        };
        sleep("2026-10-06", 7.0);
        sleep("2026-10-07", 6.0);
        sleep("2026-09-30", 8.0);
        let stats = week_stats(&conn, Week::of(d("2026-10-09"))).unwrap();
        assert_eq!(stats.completed, ["Fatura öde"]);
        assert_eq!(stats.added, 3);
        assert_eq!(stats.open_late, ["Rapor yaz"], "this week's, not every open task ever (final review M3)");
        assert_eq!(stats.next_week, [("2026-10-13".to_owned(), None, "Dişçi".to_owned())]);
        assert_eq!(stats.metrics, [MetricLine { kind: MetricKind::Sleep, this_week: Some(6.5), last_week: Some(8.0) }]);
    }

    #[test]
    fn the_report_reads_in_the_users_language() {
        let week = Week::of(d("2026-10-09"));
        let stats = WeekStats {
            completed: vec!["Fatura öde".into()],
            added: 3,
            open_late: vec!["Rapor yaz".into()],
            next_week: vec![("2026-10-13".into(), Some("10:00".into()), "Dişçi".into())],
            metrics: vec![
                MetricLine { kind: MetricKind::Sleep, this_week: Some(6.5), last_week: Some(8.0) },
                MetricLine { kind: MetricKind::Steps, this_week: Some(8420.0), last_week: None },
            ],
        };
        let tr = render(week, "tr", Ok("- Bütçe onaylandı [[Toplantı]]"), &stats);
        assert!(tr.starts_with("---\npla_generated: true\nweek: 2026-W41\n---\n# Hafta 41 · 5–11 Ekim 2026"), "{tr}");
        for want in [
            "- Bütçe onaylandı [[Toplantı]]",
            "Tamamlanan 1 · eklenen 3 · bekleyen 1",
            "| Uyku (günlük ort.) | 6,5 sa | 8 sa |",
            "| Adım (günlük ort.) | 8.420 | – |",
            "- 13 Ekim Sal 10:00 · Dişçi",
        ] {
            assert!(tr.contains(want), "{want}\n{tr}");
        }
        let en = render(week, "en", Err(NoHighlights::NoModel), &WeekStats::default());
        assert!(en.contains("# Week 41 · 5–11 October 2026") && en.contains("No summary without the language model"), "{en}");
        assert_eq!(span(Week::of(d("2026-10-01")), "tr"), "28 Eylül – 4 Ekim 2026");
        assert_eq!(span(Week::of(d("2026-12-31")), "en"), "28 December 2026 – 3 January 2027");
    }

    #[test]
    fn only_bullets_are_kept_from_the_model() {
        assert_eq!(clean_highlights("İşte özet:\n- Bir [[A]]\n* İki\n\nSon söz").as_deref(), Some("- Bir [[A]]\n- İki"));
        assert_eq!(clean_highlights("özet yok"), None);
    }

    #[test]
    fn a_report_is_never_overwritten() {
        // FR-SCH-010
        let tmp = tempfile::tempdir().unwrap();
        let vault = crate::vault::open_vault(tmp.path()).unwrap();
        let week = Week::of(d("2026-10-09"));
        assert!(!report_exists(&vault, week));
        assert_eq!(write_report(&vault, week, "first").unwrap().as_deref(), Some("reports/weekly/2026-W41.md"));
        assert_eq!(write_report(&vault, week, "second").unwrap(), None);
        assert_eq!(std::fs::read_to_string(tmp.path().join("reports/weekly/2026-W41.md")).unwrap(), "first");
        assert!(report_exists(&vault, week));
    }
}
