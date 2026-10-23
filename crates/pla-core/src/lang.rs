//! The interface language, for the texts PLA writes itself (FR-I18N-006: weekly reports, daily
//! summaries). The window's own strings live in the page (i18n.ts); the assistant answers in the
//! question's language (`qa::lang`).

use chrono::{Datelike, NaiveDate};

/// Weekday names, Monday first: the model reads the English ones, the Turkish ones are searched too.
const WEEKDAYS_EN: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const WEEKDAYS_TR: [&str; 7] = ["Pazartesi", "Salı", "Çarşamba", "Perşembe", "Cuma", "Cumartesi", "Pazar"];

/// The weekday of `date` in `lang` ("Tuesday", "Salı").
pub fn weekday_name(date: NaiveDate, lang: Lang) -> &'static str {
    let names = if lang.is_en() { &WEEKDAYS_EN } else { &WEEKDAYS_TR };
    names[date.weekday().num_days_from_monday() as usize]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    Tr,
    En,
}

impl Lang {
    /// `"tr"` or `"en"` as the settings and the page say it; anything else is English, the language
    /// PLA falls back to (FR-SET-003, FR-I18N-007).
    pub fn from_code(code: &str) -> Self {
        if code == "tr" {
            Lang::Tr
        } else {
            Lang::En
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::Tr => "tr",
            Lang::En => "en",
        }
    }

    /// The language's name in English, as the model is told to write in it.
    pub fn name(self) -> &'static str {
        match self {
            Lang::Tr => "Turkish",
            Lang::En => "English",
        }
    }

    pub fn is_en(self) -> bool {
        self == Lang::En
    }
}

#[cfg(test)]
mod tests {
    use super::{weekday_name, Lang};

    #[test]
    fn weekdays_are_named_monday_first_in_both_languages() {
        let tue = chrono::NaiveDate::from_ymd_opt(2026, 10, 6).unwrap();
        assert_eq!((weekday_name(tue, Lang::En), weekday_name(tue, Lang::Tr)), ("Tuesday", "Salı"));
        let sun = chrono::NaiveDate::from_ymd_opt(2026, 10, 11).unwrap();
        assert_eq!((weekday_name(sun, Lang::En), weekday_name(sun, Lang::Tr)), ("Sunday", "Pazar"));
    }

    #[test]
    fn a_code_names_a_language_and_the_unknown_is_english() {
        assert_eq!(Lang::from_code("tr"), Lang::Tr);
        assert_eq!(Lang::from_code("en"), Lang::En);
        assert_eq!(Lang::from_code("de"), Lang::En);
        assert_eq!(Lang::Tr.name(), "Turkish");
        assert_eq!(Lang::En.code(), "en");
    }
}
