//! The interface language, for the texts PLA writes itself (FR-I18N-006: weekly reports, daily
//! summaries). The window's own strings live in the page (i18n.ts); the assistant answers in the
//! question's language (`qa::lang`).

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
        if code == "tr" { Lang::Tr } else { Lang::En }
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
    use super::Lang;

    #[test]
    fn a_code_names_a_language_and_the_unknown_is_english() {
        assert_eq!(Lang::from_code("tr"), Lang::Tr);
        assert_eq!(Lang::from_code("en"), Lang::En);
        assert_eq!(Lang::from_code("de"), Lang::En);
        assert_eq!(Lang::Tr.name(), "Turkish");
        assert_eq!(Lang::En.code(), "en");
    }
}
