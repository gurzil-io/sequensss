//! Interface language (French / English).
//!
//! Strings are written as pairs right where they are used:
//! `tr!("Enregistrer", "Save")` (`&'static str`), or formatted with
//! `trf!("{n} notes gravées", "{n} notes baked")` (`String`).
//! The current language is global and can be switched live (applied on the next frame).

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Fr,
    En,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::Fr, Lang::En];

    /// Short code (settings file).
    pub fn code(self) -> &'static str {
        match self {
            Lang::Fr => "fr",
            Lang::En => "en",
        }
    }

    /// Name of the language in that language (menu).
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::Fr => "Français",
            Lang::En => "English",
        }
    }

    /// `fr`, `fr-FR`, `fr_CA`… → French; everything else → English.
    pub fn from_locale(locale: &str) -> Lang {
        if locale.trim().to_ascii_lowercase().starts_with("fr") { Lang::Fr } else { Lang::En }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.code() == code.trim())
    }
}

static LANG: AtomicU8 = AtomicU8::new(0);

pub fn set(lang: Lang) {
    LANG.store(lang as u8, Ordering::Relaxed);
}

pub fn get() -> Lang {
    match LANG.load(Ordering::Relaxed) {
        0 => Lang::Fr,
        _ => Lang::En,
    }
}

/// Evaluates `f` in language `lang`, then restores the current language (UI thread only:
/// e.g. to recognize a default name whatever language it was created in).
pub fn with_lang<T>(lang: Lang, f: impl FnOnce() -> T) -> T {
    let prev = get();
    set(lang);
    let r = f();
    set(prev);
    r
}

/// Picks the text for the current language.
#[inline]
pub fn pick<T>(fr: T, en: T) -> T {
    match get() {
        Lang::Fr => fr,
        Lang::En => en,
    }
}

/// Fixed text: `tr!("Enregistrer", "Save")` → `&'static str` (no formatting: `{x}` is kept as is).
#[macro_export]
macro_rules! tr {
    ($fr:literal, $en:literal $(,)?) => {
        $crate::pick($fr, $en)
    };
}

/// Formatted text (like `format!`, implicit captures included) → `String`:
/// `trf!("{n} notes gravées", "{n} notes baked")`, `trf!("{} pistes", "{} tracks", k)`.
#[macro_export]
macro_rules! trf {
    ($fr:literal, $en:literal $(, $($args:tt)*)?) => {
        match $crate::get() {
            $crate::Lang::Fr => format!($fr $(, $($args)*)?),
            $crate::Lang::En => format!($en $(, $($args)*)?),
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_and_format() {
        set(Lang::En);
        assert_eq!(tr!("Enregistrer", "Save"), "Save");
        let n = 3;
        assert_eq!(trf!("{n} notes gravées", "{n} notes baked"), "3 notes baked");
        assert_eq!(trf!("{} pistes", "{} tracks", 2), "2 tracks");
        assert_eq!(trf!("{a}/{b}", "{a} of {b}", a = 1, b = 4), "1 of 4");
        set(Lang::Fr);
        assert_eq!(tr!("Enregistrer", "Save"), "Enregistrer");
        assert_eq!(Lang::from_locale("fr-FR"), Lang::Fr);
        assert_eq!(Lang::from_locale("en_US"), Lang::En);
        assert_eq!(Lang::from_locale("de"), Lang::En);
        assert_eq!(Lang::from_code("en"), Some(Lang::En));
    }
}
