//! Checks every `tr!` / `trf!` pair in the workspace sources: both texts present, same
//! placeholders, same surrounding whitespace and trailing punctuation.

use std::path::{Path, PathBuf};

/// A `tr!` / `trf!` call found in the sources.
struct Call {
    at: String,
    fr: String,
    en: String,
}

#[test]
fn translation_pairs_are_consistent() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(crates).unwrap() {
        rs_files(&entry.unwrap().path().join("src"), &mut files);
    }
    files.sort();

    let mut calls = Vec::new();
    let mut errors = Vec::new();
    for f in &files {
        let src = std::fs::read_to_string(f).unwrap();
        let rel = f.strip_prefix(crates).unwrap_or(f).display().to_string();
        match scan(&src) {
            Ok(found) => calls.extend(found.into_iter().map(|(line, fr, en)| Call { at: format!("{rel}:{line}"), fr, en })),
            Err((line, e)) => errors.push(format!("{rel}:{line}: {e}")),
        }
    }
    assert!(calls.len() > 100, "only {} calls found: parser broken?", calls.len());

    for c in &calls {
        errors.extend(check(&c.fr, &c.en).into_iter().map(|e| format!("{}: {e}\n    fr: {:?}\n    en: {:?}", c.at, c.fr, c.en)));
    }
    assert!(errors.is_empty(), "{} problem(s):\n{}", errors.len(), errors.join("\n"));
}

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

// ---------------------------------------------------------------------------
// Checks
// ---------------------------------------------------------------------------

fn check(fr: &str, en: &str) -> Vec<String> {
    let mut errors = Vec::new();
    if fr.trim().is_empty() || en.trim().is_empty() {
        errors.push("empty text".to_string());
    }
    match (placeholders(fr), placeholders(en)) {
        (Ok((mut fr_named, fr_pos)), Ok((mut en_named, en_pos))) => {
            fr_named.sort();
            fr_named.dedup();
            en_named.sort();
            en_named.dedup();
            if fr_named != en_named {
                errors.push(format!("named placeholders differ: {fr_named:?} vs {en_named:?}"));
            }
            if fr_pos != en_pos {
                errors.push(format!("positional placeholders differ: {fr_pos:?} vs {en_pos:?}"));
            }
        }
        (Err(e), _) | (_, Err(e)) => errors.push(e),
    }
    let starts_ws = |s: &str| s.starts_with(char::is_whitespace);
    let ends_ws = |s: &str| s.ends_with(char::is_whitespace);
    if starts_ws(fr) != starts_ws(en) {
        errors.push("leading whitespace differs".to_string());
    }
    if ends_ws(fr) != ends_ws(en) {
        errors.push("trailing whitespace differs".to_string());
    }
    let (f, e) = (trailing_punct(fr), trailing_punct(en));
    if f != e {
        errors.push(format!("trailing punctuation differs: {f:?} vs {e:?}"));
    }
    errors
}

/// Final punctuation mark that must match in both languages (French puts a space before
/// `:`, `?` and `!`, so the text is trimmed first). `...` counts as `…`.
fn trailing_punct(s: &str) -> Option<char> {
    let s = s.trim_end();
    if s.ends_with("...") {
        return Some('…');
    }
    s.chars().last().filter(|c| "…:?!".contains(*c))
}

/// Format placeholders: (named / indexed ones with their spec, specs of the positional `{}`
/// in order). `{{` and `}}` are escapes.
fn placeholders(s: &str) -> Result<(Vec<String>, Vec<String>), String> {
    let (mut named, mut positional) = (Vec::new(), Vec::new());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '{' if it.peek() == Some(&'{') => {
                it.next();
            }
            '}' if it.peek() == Some(&'}') => {
                it.next();
            }
            '{' => {
                let mut inner = String::new();
                loop {
                    match it.next() {
                        Some('}') => break,
                        Some('{') | None => return Err(format!("unclosed placeholder in {s:?}")),
                        Some(c) => inner.push(c),
                    }
                }
                let (name, spec) = inner.split_once(':').unwrap_or((&inner, ""));
                if name.trim().is_empty() {
                    positional.push(spec.to_string());
                } else {
                    named.push(inner.clone());
                }
            }
            '}' => return Err(format!("unmatched '}}' in {s:?}")),
            _ => {}
        }
    }
    Ok((named, positional))
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// (line, French text, English text).
type Found = (usize, String, String);

/// Finds `tr!(…)` / `trf!(…)` calls in Rust code (comments and string contents are skipped)
/// Errors: (line, message).
fn scan(src: &str) -> Result<Vec<Found>, (usize, String)> {
    let mut l = Lexer { s: src.chars().collect(), i: 0 };
    let mut out = Vec::new();
    while let Some(c) = l.peek(0) {
        if l.skip_comment() {
            continue;
        }
        if c == '"' || (c == 'r' || c == 'b') && l.starts_string() {
            let line = l.line();
            l.string().map_err(|e| (line, e))?;
        } else if c == '\'' {
            l.char_or_lifetime();
        } else if c.is_alphanumeric() || c == '_' {
            let start = l.i;
            let ident = l.ident();
            if (ident == "tr" || ident == "trf") && l.after_ws_is("!") {
                l.skip_ws();
                l.i += 1;
                l.skip_ws();
                // `macro_rules! tr { … }` has no `!` right after the name, so isn't matched.
                let line = l.line_at(start);
                let err = |e: String| (line, format!("{ident}!: {e}"));
                if !matches!(l.peek(0), Some('(' | '[' | '{')) {
                    return Err(err("expected an opening bracket".into()));
                }
                l.i += 1;
                l.skip_ws();
                let fr = l.string().map_err(err)?;
                l.skip_ws();
                if l.peek(0) != Some(',') {
                    return Err(err("expected ',' after the French text".into()));
                }
                l.i += 1;
                l.skip_ws();
                let en = l.string().map_err(err)?;
                out.push((line, fr, en));
            }
        } else {
            l.i += 1;
        }
    }
    Ok(out)
}

struct Lexer {
    s: Vec<char>,
    i: usize,
}

impl Lexer {
    fn peek(&self, k: usize) -> Option<char> {
        self.s.get(self.i + k).copied()
    }

    fn line(&self) -> usize {
        self.line_at(self.i)
    }

    fn line_at(&self, i: usize) -> usize {
        self.s[..i].iter().filter(|&&c| c == '\n').count() + 1
    }

    fn ident(&mut self) -> String {
        let start = self.i;
        while self.peek(0).is_some_and(|c| c.is_alphanumeric() || c == '_') {
            self.i += 1;
        }
        self.s[start..self.i].iter().collect()
    }

    /// Skips whitespace and comments.
    fn skip_ws(&mut self) {
        loop {
            if self.peek(0).is_some_and(char::is_whitespace) {
                self.i += 1;
            } else if !self.skip_comment() {
                return;
            }
        }
    }

    fn after_ws_is(&mut self, tok: &str) -> bool {
        let save = self.i;
        self.skip_ws();
        let ok = tok.chars().enumerate().all(|(k, c)| self.peek(k) == Some(c));
        self.i = save;
        ok
    }

    fn skip_comment(&mut self) -> bool {
        match (self.peek(0), self.peek(1)) {
            (Some('/'), Some('/')) => {
                while self.peek(0).is_some_and(|c| c != '\n') {
                    self.i += 1;
                }
                true
            }
            (Some('/'), Some('*')) => {
                self.i += 2;
                let mut depth = 1;
                while depth > 0 && self.peek(0).is_some() {
                    match (self.peek(0), self.peek(1)) {
                        (Some('/'), Some('*')) => depth += 1,
                        (Some('*'), Some('/')) => depth -= 1,
                        _ => {
                            self.i += 1;
                            continue;
                        }
                    }
                    self.i += 2;
                }
                true
            }
            _ => false,
        }
    }

    /// At `r"`, `r#…"`, `b"`, `br"`… (and not an identifier such as `r#type` or `bar`).
    fn starts_string(&self) -> bool {
        let mut k = 0;
        if self.peek(k) == Some('b') {
            k += 1;
        }
        if self.peek(k) == Some('r') {
            k += 1;
            while self.peek(k) == Some('#') {
                k += 1;
            }
        }
        k > 0 && self.peek(k) == Some('"')
    }

    /// Skips a char literal (`'a'`, `'\n'`, `'\u{..}'`) or a lifetime (`'a`).
    fn char_or_lifetime(&mut self) {
        self.i += 1;
        if self.peek(0) == Some('\\') {
            self.i += 2;
            while self.peek(0).is_some_and(|c| c != '\'') {
                self.i += 1;
            }
            self.i += 1;
        } else if self.peek(1) == Some('\'') {
            self.i += 2;
        }
    }

    /// Reads a string literal (plain or raw) and returns its value.
    fn string(&mut self) -> Result<String, String> {
        if self.peek(0) == Some('b') {
            self.i += 1;
        }
        if self.peek(0) == Some('r') {
            self.i += 1;
            let mut hashes = 0;
            while self.peek(0) == Some('#') {
                hashes += 1;
                self.i += 1;
            }
            if self.peek(0) != Some('"') {
                return Err("expected a string literal".into());
            }
            self.i += 1;
            let close: Vec<char> = std::iter::once('"').chain(std::iter::repeat_n('#', hashes)).collect();
            let start = self.i;
            while self.i < self.s.len() {
                if self.s[self.i..].starts_with(&close) {
                    let v = self.s[start..self.i].iter().collect();
                    self.i += close.len();
                    return Ok(v);
                }
                self.i += 1;
            }
            return Err("unterminated raw string".into());
        }
        if self.peek(0) != Some('"') {
            return Err(format!("expected a string literal, found {:?}", self.peek(0)));
        }
        self.i += 1;
        let mut v = String::new();
        loop {
            let Some(c) = self.peek(0) else { return Err("unterminated string".into()) };
            self.i += 1;
            match c {
                '"' => return Ok(v),
                '\\' => {
                    let Some(e) = self.peek(0) else { return Err("unterminated string".into()) };
                    self.i += 1;
                    match e {
                        'n' => v.push('\n'),
                        't' => v.push('\t'),
                        'r' => v.push('\r'),
                        '0' => v.push('\0'),
                        '\\' | '"' | '\'' => v.push(e),
                        '\n' | '\r' => {
                            // Line continuation: the newline and the next line's indentation are dropped.
                            while self.peek(0).is_some_and(char::is_whitespace) {
                                self.i += 1;
                            }
                        }
                        'x' => {
                            let hex: String = self.s[self.i..(self.i + 2).min(self.s.len())].iter().collect();
                            self.i += 2;
                            v.push(u8::from_str_radix(&hex, 16).map_err(|_| format!("bad escape \\x{hex}"))? as char);
                        }
                        'u' => {
                            let end = self.s[self.i..].iter().position(|&c| c == '}').ok_or("bad \\u escape")? + self.i;
                            let hex: String = self.s[self.i + 1..end].iter().collect();
                            self.i = end + 1;
                            let c = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32);
                            v.push(c.ok_or(format!("bad escape \\u{{{hex}}}"))?);
                        }
                        _ => return Err(format!("unknown escape \\{e}")),
                    }
                }
                _ => v.push(c),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests of the test helpers
// ---------------------------------------------------------------------------

#[test]
fn parser_handles_escapes_and_continuations() {
    let src = r#####"
        // tr!("ignored", "in a comment")
        let s = "tr!(\"ignored\", \"in a string\")";
        let c = '"';
        fn f<'a>(x: &'a str) {}
        tr!("Il a dit \"oui\"", "He said \"yes\"");
        seq_i18n::trf!(
            "Ligne un\n\
             ligne {n}",
            "Line one\n\
             line {n}",
        );
        tr!(r#"brut "x""#, r"raw");
        macro_rules! tr { ($a:literal) => {}; }
        /* tr!("block", "comment") */
    "#####;
    let calls = scan(src).unwrap();
    let pairs: Vec<(&str, &str)> = calls.iter().map(|(_, f, e)| (f.as_str(), e.as_str())).collect();
    assert_eq!(
        pairs,
        vec![
            ("Il a dit \"oui\"", "He said \"yes\""),
            ("Ligne un\nligne {n}", "Line one\nline {n}"),
            (r#"brut "x""#, "raw"),
        ]
    );
    assert_eq!(calls[1].0, 7);
}

#[test]
fn checks_catch_mismatches() {
    assert!(check("Ouvrir…", "Open…").is_empty());
    assert!(check("Audio : {e}", "Audio: {e}").is_empty());
    assert!(check("{} pistes ({n:.1} s)", "{} tracks ({n:.1} s)").is_empty());
    assert!(check("{{littéral}}", "{{literal}}").is_empty());
    assert!(check("Enregistrer sous :", "Save as:").is_empty());
    assert!(check("Supprimer ?", "Delete?").is_empty());
    assert_eq!(check("", "Save").len(), 1);
    assert_eq!(check("{n} notes", "{count} notes").len(), 1);
    assert_eq!(check("{} et {}", "{} and").len(), 1);
    assert_eq!(check("{:.1} s", "{} s").len(), 1);
    assert_eq!(check("Ouvrir…", "Open").len(), 1);
    assert_eq!(check("Nom :", "Name").len(), 1);
    assert_eq!(check(" Ouvrir", "Open").len(), 1);
    assert_eq!(check("Ouvrir ", "Open").len(), 1);
    assert!(!check("{n", "{n}").is_empty());
}
