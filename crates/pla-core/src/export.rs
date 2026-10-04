//! Tasks and metrics as CSV for Privacy & Data (NFR-SEC-008). Written for Excel: a UTF-8 BOM so
//! Turkish letters survive, and the separator and decimal mark of the user's Windows region, so a
//! Turkish Excel (`;` and `,`) splits the columns too.

use rusqlite::Connection;

/// The separator and decimal mark the user's spreadsheet expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CsvStyle {
    pub separator: char,
    pub decimal: char,
}

impl Default for CsvStyle {
    fn default() -> Self {
        CsvStyle { separator: ',', decimal: '.' }
    }
}

struct Words {
    task_head: [&'static str; 10],
    metric_head: [&'static str; 7],
    open: &'static str,
    done: &'static str,
    cancelled: &'static str,
    manual: &'static str,
    assistant: &'static str,
    metric: fn(&str) -> &'static str,
    sets: &'static str,
    reps: &'static str,
}

const TR: Words = Words {
    task_head: ["Başlık", "Ayrıntı", "Tarih", "Saat", "Hatırlatma", "Durum", "Kaynak", "Not", "Tamamlanma", "Oluşturulma"],
    metric_head: ["Tarih", "Tür", "Değer", "Birim", "Ayrıntı", "Kaynak", "Not"],
    open: "açık",
    done: "tamamlandı",
    cancelled: "iptal",
    manual: "elle",
    assistant: "asistan",
    metric: |t| match t {
        "sleep" => "uyku",
        "weight" => "kilo",
        "steps" => "adım",
        "water" => "su",
        "workout" => "egzersiz",
        _ => "?",
    },
    sets: "set",
    reps: "tekrar",
};

const EN: Words = Words {
    task_head: ["Title", "Details", "Date", "Time", "Reminder", "Status", "Origin", "Note", "Completed", "Created"],
    metric_head: ["Date", "Type", "Value", "Unit", "Details", "Origin", "Note"],
    open: "open",
    done: "done",
    cancelled: "cancelled",
    manual: "manual",
    assistant: "assistant",
    metric: |t| match t {
        "sleep" => "sleep",
        "weight" => "weight",
        "steps" => "steps",
        "water" => "water",
        "workout" => "workout",
        _ => "?",
    },
    sets: "sets",
    reps: "reps",
};

fn words(lang: &str) -> &'static Words {
    if lang == "tr" {
        &TR
    } else {
        &EN
    }
}

fn origin(w: &Words, o: &str) -> &'static str {
    match o {
        "extracted" => "AI",
        "assistant" => w.assistant,
        _ => w.manual,
    }
}

/// One cell: text a spreadsheet would run as a formula gets a leading `'`, and a cell holding the
/// separator, a quote or a line break is quoted (RFC 4180).
fn cell(text: &str, style: CsvStyle) -> String {
    let text = if text.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{text}") } else { text.to_owned() };
    if text.contains([style.separator, '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

fn row(out: &mut String, cells: &[String], style: CsvStyle) {
    let line: Vec<String> = cells.iter().map(|c| cell(c, style)).collect();
    out.push_str(&line.join(&style.separator.to_string()));
    out.push_str("\r\n");
}

fn number(v: f64, style: CsvStyle) -> String {
    let text = if v.fract() == 0.0 && v.abs() < 1e15 { format!("{}", v as i64) } else { v.to_string() };
    text.replace('.', &style.decimal.to_string())
}

pub fn tasks_csv(conn: &Connection, lang: &str, style: CsvStyle) -> rusqlite::Result<String> {
    let w = words(lang);
    let mut out = String::from('\u{feff}');
    row(&mut out, &w.task_head.map(String::from), style);
    let mut stmt = conn.prepare(
        "SELECT t.title, t.details, t.date, t.time, t.notify_at, t.status, t.origin, b.note_path, t.completed_at, t.created_at
         FROM task t LEFT JOIN block b ON b.block_id = t.block_id ORDER BY t.created_at, t.rowid",
    )?;
    let rows = stmt.query_map([], |r| {
        let get = |i: usize| r.get::<_, Option<String>>(i).map(Option::unwrap_or_default);
        let status = match r.get::<_, String>(5)?.as_str() {
            "done" => w.done,
            "cancelled" => w.cancelled,
            _ => w.open,
        };
        let source = origin(w, &r.get::<_, String>(6)?);
        Ok(vec![get(0)?, get(1)?, get(2)?, get(3)?, get(4)?, status.to_owned(), source.to_owned(), get(7)?, get(8)?, get(9)?])
    })?;
    for cells in rows {
        row(&mut out, &cells?, style);
    }
    Ok(out)
}

pub fn metrics_csv(conn: &Connection, lang: &str, style: CsvStyle) -> rusqlite::Result<String> {
    let w = words(lang);
    let mut out = String::from('\u{feff}');
    row(&mut out, &w.metric_head.map(String::from), style);
    let mut stmt = conn.prepare(
        "SELECT m.date, m.type, m.value_json, m.unit, m.origin, b.note_path
         FROM metric_record m LEFT JOIN block b ON b.block_id = m.block_id ORDER BY m.date, m.created_at, m.rowid",
    )?;
    let rows = stmt.query_map([], |r| {
        let kind: String = r.get(1)?;
        let json: serde_json::Value = serde_json::from_str(&r.get::<_, String>(2)?).unwrap_or_default();
        let value = json.get("value").and_then(|v| v.as_f64()).map(|v| number(v, style)).unwrap_or_default();
        let mut detail: Vec<String> = Vec::new();
        if let Some(e) = json.get("exercise").and_then(|v| v.as_str()) {
            detail.push(e.to_owned());
        }
        match (json.get("sets").and_then(|v| v.as_i64()), json.get("reps").and_then(|v| v.as_i64())) {
            (Some(s), Some(n)) => detail.push(format!("{s}×{n}")),
            (Some(s), None) => detail.push(format!("{s} {}", w.sets)),
            (None, Some(n)) => detail.push(format!("{n} {}", w.reps)),
            (None, None) => {}
        }
        Ok(vec![
            r.get::<_, String>(0)?,
            (w.metric)(&kind).to_owned(),
            value,
            r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            detail.join(" "),
            origin(w, &r.get::<_, String>(4)?).to_owned(),
            r.get::<_, Option<String>>(5)?.unwrap_or_default(),
        ])
    })?;
    for cells in rows {
        row(&mut out, &cells?, style);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_databases;

    const NOW: &str = "2026-10-06T10:00:00+03:00";

    fn setup() -> (tempfile::TempDir, Connection) {
        let tmp = tempfile::tempdir().unwrap();
        let conn = open_databases(tmp.path()).unwrap().pla;
        conn.execute(
            "INSERT INTO block (block_id, note_path, position, text, text_hash, first_seen_at, last_seen_at) VALUES ('b1', 'daily/2026/2026-10-06.md', 0, 'x', 'h', ?1, ?1)",
            [NOW],
        )
        .unwrap();
        let task = "INSERT INTO task (task_id, title, details, date, time, notify_at, status, origin, block_id, completed_at, created_at, updated_at)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)";
        conn.execute(task, rusqlite::params!["t1", "Dişçi", None::<String>, "2026-10-07", "09:00", "2026-10-07T09:00", "open", "extracted", "b1", None::<String>, NOW]).unwrap();
        conn.execute(task, rusqlite::params!["t2", "Fatura, \"elektrik\"", "satır 1\nsatır 2", None::<String>, None::<String>, None::<String>, "done", "manual", None::<String>, NOW, "2026-10-05T08:00:00+03:00"]).unwrap();
        let metric = "INSERT INTO metric_record (metric_id, type, value_json, unit, date, origin, block_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)";
        conn.execute(metric, rusqlite::params!["m1", "sleep", r#"{"value":7.5}"#, "h", "2026-10-05", "extracted", "b1", NOW]).unwrap();
        conn.execute(metric, rusqlite::params!["m2", "workout", r#"{"exercise":"şınav","sets":3,"reps":10}"#, None::<String>, "2026-10-06", "manual", None::<String>, NOW]).unwrap();
        (tmp, conn)
    }

    fn lines(csv: &str) -> Vec<&str> {
        csv.trim_start_matches('\u{feff}').split("\r\n").filter(|l| !l.is_empty()).collect()
    }

    #[test]
    fn tasks_export_for_excel_with_quoting_and_the_source_note() {
        let (_tmp, conn) = setup();
        let csv = tasks_csv(&conn, "tr", CsvStyle::default()).unwrap();
        assert!(csv.starts_with('\u{feff}'), "BOM so Excel reads UTF-8");
        let rows = lines(&csv);
        assert_eq!(rows[0], "Başlık,Ayrıntı,Tarih,Saat,Hatırlatma,Durum,Kaynak,Not,Tamamlanma,Oluşturulma");
        assert_eq!(rows[2], "Dişçi,,2026-10-07,09:00,2026-10-07T09:00,açık,AI,daily/2026/2026-10-06.md,,2026-10-06T10:00:00+03:00");
        // oldest first; a comma, quotes and a line break stay inside one quoted cell
        assert!(csv.contains("\"Fatura, \"\"elektrik\"\"\",\"satır 1\nsatır 2\",,,,tamamlandı,elle,,"), "{csv}");
        let en = tasks_csv(&conn, "en", CsvStyle::default()).unwrap();
        assert!(lines(&en)[0].starts_with("Title,Details,Date,Time,Reminder,Status,Origin,Note"));
        assert!(en.contains(",open,AI,"));
    }

    #[test]
    fn metrics_export_uses_the_regions_separator_and_decimal_mark() {
        let (_tmp, conn) = setup();
        let turkish = CsvStyle { separator: ';', decimal: ',' };
        let rows_owned = metrics_csv(&conn, "tr", turkish).unwrap();
        let rows = lines(&rows_owned);
        assert_eq!(rows[0], "Tarih;Tür;Değer;Birim;Ayrıntı;Kaynak;Not");
        assert_eq!(rows[1], "2026-10-05;uyku;7,5;h;;AI;daily/2026/2026-10-06.md");
        assert_eq!(rows[2], "2026-10-06;egzersiz;;;şınav 3×10;elle;");
        let en = metrics_csv(&conn, "en", CsvStyle::default()).unwrap();
        assert_eq!(lines(&en)[1], "2026-10-05,sleep,7.5,h,,AI,daily/2026/2026-10-06.md");
    }

    #[test]
    fn a_cell_that_holds_the_separator_is_quoted() {
        let (_tmp, conn) = setup();
        conn.execute("UPDATE task SET title = 'a;b' WHERE task_id = 't1'", []).unwrap();
        let csv = tasks_csv(&conn, "tr", CsvStyle { separator: ';', decimal: ',' }).unwrap();
        assert!(lines(&csv)[2].starts_with("\"a;b\";"), "{csv}");
    }

    #[test]
    fn formulas_are_not_run_by_the_spreadsheet() {
        // CSV injection: a title starting with = + - @ would be a formula in Excel
        let (_tmp, conn) = setup();
        conn.execute("UPDATE task SET title = '=HYPERLINK(\"x\")' WHERE task_id = 't1'", []).unwrap();
        let csv = tasks_csv(&conn, "en", CsvStyle::default()).unwrap();
        assert!(lines(&csv)[2].starts_with("\"'=HYPERLINK(\"\"x\"\")\""), "{csv}");
    }
}
