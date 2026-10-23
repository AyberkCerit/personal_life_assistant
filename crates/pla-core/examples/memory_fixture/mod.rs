//! Shared by `nl_eval` and `retrieval_eval`: a case's `memory` JSON written as the memory files.

/// Writes a case's `memory` object (`{about, rules, state, people: {Name: [..]}}`; items
/// `{text, kind: said|inferred|manual, source, date}`) into `dir` as the memory files.
pub fn write_memory(dir: &std::path::Path, mem: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    use chrono::NaiveDate;
    use pla_core::lang::Lang;
    use pla_core::recall::{render, topic_key, Entry, Kind, Role};
    let item = |v: &serde_json::Value, person: Option<&str>| -> Entry {
        let text = v["text"].as_str().unwrap().to_owned();
        let kind = match v["kind"].as_str().unwrap_or("said") {
            "inferred" => Kind::Inferred,
            "manual" => Kind::Manual,
            _ => Kind::Said,
        };
        let date = v["date"].as_str().and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()).unwrap_or(NaiveDate::MIN);
        Entry {
            id: pla_core::util::content_hash(topic_key(&text).as_bytes()).chars().take(8).collect(),
            topic: topic_key(&text).split('-').take(4).collect::<Vec<_>>().join("-"),
            text,
            kind,
            source: v["source"].as_str().map(str::to_owned),
            date,
            person: person.map(str::to_owned),
            notes: Vec::new(),
        }
    };
    std::fs::create_dir_all(dir)?;
    for (field, role) in [("about", Role::About), ("rules", Role::Rules), ("state", Role::State)] {
        if let Some(list) = mem[field].as_array() {
            let entries: Vec<Entry> = list.iter().map(|v| item(v, None)).collect();
            std::fs::write(dir.join(role.file_name(Lang::Tr)), render(role, &entries, Lang::Tr))?;
        }
    }
    if let Some(people) = mem["people"].as_object() {
        let mut entries = Vec::new();
        for (name, list) in people {
            entries.extend(list.as_array().into_iter().flatten().map(|v| item(v, Some(name))));
        }
        std::fs::write(dir.join(Role::People.file_name(Lang::Tr)), render(Role::People, &entries, Lang::Tr))?;
    }
    Ok(())
}
