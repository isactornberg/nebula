//! A change to one key of a settings file, in its text: Claude's JSON is
//! spliced where the key is, so every other byte of the file stays as it
//! was, and Codex's TOML goes through `toml_edit`, which keeps the comments
//! and layout of what it does not change. A file that does not parse is
//! never changed, and the changed text is parsed again and held to the one
//! change before anything writes it ([`json_checked`], [`codex_checked`]).

use std::ops::Range;
use std::path::{Path, PathBuf};
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table};

/// Why a settings file's text was not changed.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Unedited {
    /// The file cannot take the change as it stands, as the end of a
    /// sentence about it: `is not valid JSON: …`.
    Refused(String),
    /// The changed text, parsed again, says more or less than the change
    /// to `.0`.
    Unsafe(String),
}

impl From<String> for Unedited {
    fn from(why: String) -> Self {
        Unedited::Refused(why)
    }
}

/// [`json_set`], and then [`json_checked`] on what it made.
pub(super) fn json_edit(
    text: &str,
    outer: &str,
    key: &str,
    value: Option<&str>,
) -> Result<String, Unedited> {
    let new = json_set(text, outer, key, value)?;
    json_checked(text, new, outer, key, value)
}

/// `new`, when it parses and is `old` with only `outer.key` set to the
/// JSON `value` (removed for `None`), as values: an `outer` made for the
/// key, or left empty without it, included. An empty `old` is `{}`.
pub(super) fn json_checked(
    old: &str,
    new: String,
    outer: &str,
    key: &str,
    value: Option<&str>,
) -> Result<String, Unedited> {
    if new == old {
        return Ok(new);
    }
    let parse = |t: &str| -> Option<serde_json::Value> {
        if t.trim().is_empty() {
            Some(serde_json::Value::Object(serde_json::Map::new()))
        } else {
            serde_json::from_str(t).ok()
        }
    };
    let unsafe_edit = || Unedited::Unsafe(format!("`{outer}.{key}`"));
    let (Some(mut want), Some(got)) = (parse(old), parse(&new)) else {
        return Err(unsafe_edit());
    };
    let top = want.as_object_mut().ok_or_else(unsafe_edit)?;
    match value {
        Some(v) => {
            let v: serde_json::Value = serde_json::from_str(v).map_err(|_| unsafe_edit())?;
            let inner = top
                .entry(outer)
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
            inner
                .as_object_mut()
                .ok_or_else(unsafe_edit)?
                .insert(key.to_string(), v);
        }
        None => {
            if let Some(inner) = top.get_mut(outer).and_then(|v| v.as_object_mut()) {
                inner.remove(key);
            }
        }
    }
    if got == want {
        Ok(new)
    } else {
        Err(unsafe_edit())
    }
}

/// `text` with `outer.key` set to the JSON `value`, or with that entry
/// removed when `value` is `None`; the same text when nothing changes. An
/// empty `text` is a file that is not there yet, which gets only that key.
pub(super) fn json_set(
    text: &str,
    outer: &str,
    key: &str,
    value: Option<&str>,
) -> Result<String, String> {
    let fresh;
    let text = if text.trim().is_empty() {
        if value.is_none() {
            return Ok(text.to_string());
        }
        fresh = "{\n}\n".to_string();
        &fresh
    } else {
        text
    };
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(v) if v.is_object() => {}
        Ok(_) => return Err("does not hold a JSON object".into()),
        Err(e) => return Err(format!("is not valid JSON: {e}")),
    }
    let top = object(text, skip_ws(text.as_bytes(), 0)).ok_or("is not valid JSON")?;
    let style = Style::of(text, &top);
    let Some(at) = top.last(outer) else {
        let Some(value) = value else {
            return Ok(text.to_string());
        };
        let text = insert(text, &top, outer, "{}", &style);
        return json_set(&text, outer, key, Some(value));
    };
    let at = top.members[at].value.clone();
    if text.as_bytes()[at.start] != b'{' {
        return Err(format!("has a `{outer}` that is not a JSON object"));
    }
    let inner = object(text, at.start).ok_or("is not valid JSON")?;
    Ok(match (value, inner.last(key)) {
        (Some(value), Some(i)) => {
            let span = inner.members[i].value.clone();
            format!("{}{value}{}", &text[..span.start], &text[span.end..])
        }
        (Some(value), None) => insert(text, &inner, key, value, &style),
        (None, Some(_)) => {
            // Every entry under the name, so none is left to win.
            let mut text = text.to_string();
            while let Some(inner) = object(&text, at.start) {
                let Some(i) = inner.last(key) else { break };
                text = remove(&text, &inner, i);
            }
            text
        }
        (None, None) => text.to_string(),
    })
}

/// What `outer.key` holds in `text`, as a string, if it is one.
pub(super) fn json_string(text: &str, outer: &str, key: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    v.get(outer)?.get(key)?.as_str().map(String::from)
}

/// A JSON string literal of `s`.
pub(super) fn quoted(s: &str) -> String {
    serde_json::to_string(s).expect("a string serializes")
}

/// An object's members where they lie in the text: `open` and `close` are
/// its braces.
struct Object {
    open: usize,
    close: usize,
    members: Vec<Member>,
}

struct Member {
    key: String,
    /// From the key's opening quote.
    start: usize,
    /// Between the key and its value: `": "`.
    colon: Range<usize>,
    value: Range<usize>,
}

impl Object {
    fn last(&self, key: &str) -> Option<usize> {
        self.members.iter().rposition(|m| m.key == key)
    }
}

/// How the file lays out an object: whether it puts members on lines of
/// their own, the indent of one level, and what is between a key and its
/// value.
struct Style {
    lines: bool,
    indent: String,
    colon: String,
}

impl Style {
    fn of(text: &str, top: &Object) -> Style {
        let inside = &text[top.open..top.close];
        let indent = top
            .members
            .first()
            .and_then(|m| text[..m.start].rsplit_once('\n'))
            .map(|(_, lead)| lead.to_string())
            .filter(|lead| !lead.is_empty() && lead.trim().is_empty())
            .unwrap_or_else(|| "  ".into());
        Style {
            // An empty file is laid out as Claude Code writes one.
            lines: inside.contains('\n') || top.members.is_empty(),
            indent,
            colon: top
                .members
                .first()
                .map_or_else(|| ": ".into(), |m| text[m.colon.clone()].to_string()),
        }
    }
}

fn skip_ws(b: &[u8], mut at: usize) -> usize {
    while at < b.len() && b[at].is_ascii_whitespace() {
        at += 1;
    }
    at
}

/// The end of the string whose opening quote is at `at`.
fn string_end(b: &[u8], mut at: usize) -> Option<usize> {
    at += 1;
    while at < b.len() {
        match b[at] {
            b'\\' => at += 2,
            b'"' => return Some(at + 1),
            _ => at += 1,
        }
    }
    None
}

/// The end of the value that starts at `at`.
fn value_end(b: &[u8], at: usize) -> Option<usize> {
    match *b.get(at)? {
        b'"' => string_end(b, at),
        b'{' | b'[' => {
            let mut depth = 0usize;
            let mut i = at;
            while i < b.len() {
                match b[i] {
                    b'"' => {
                        i = string_end(b, i)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(i + 1);
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            None
        }
        _ => {
            let mut i = at;
            while i < b.len() && !matches!(b[i], b',' | b'}' | b']') && !b[i].is_ascii_whitespace()
            {
                i += 1;
            }
            Some(i)
        }
    }
}

/// The object whose `{` is at `at`, in text that parses as JSON.
fn object(text: &str, at: usize) -> Option<Object> {
    let b = text.as_bytes();
    if b.get(at) != Some(&b'{') {
        return None;
    }
    let mut members = Vec::new();
    let mut i = at + 1;
    loop {
        i = skip_ws(b, i);
        match *b.get(i)? {
            b'}' => {
                return Some(Object {
                    open: at,
                    close: i,
                    members,
                })
            }
            b',' => i += 1,
            b'"' => {
                let key_end = string_end(b, i)?;
                let key: String = serde_json::from_str(&text[i..key_end]).ok()?;
                let colon = skip_ws(b, key_end);
                let start = skip_ws(b, colon + 1);
                let end = value_end(b, start)?;
                members.push(Member {
                    key,
                    start: i,
                    colon: key_end..start,
                    value: start..end,
                });
                i = end;
            }
            _ => return None,
        }
    }
}

/// The blanks a line starts with, for the line `at` is on.
fn line_indent(text: &str, at: usize) -> &str {
    let line = text[..at].rfind('\n').map_or(0, |n| n + 1);
    let rest = &text[line..];
    &rest[..rest.len() - rest.trim_start_matches([' ', '\t']).len()]
}

/// `text` with `"key": value` after the last member of `obj`, laid out as
/// that member is, or as `style` says in an empty object.
fn insert(text: &str, obj: &Object, key: &str, value: &str, style: &Style) -> String {
    let key = quoted(key);
    match obj.members.last() {
        Some(last) => {
            let lead = &text[..last.start];
            let lead = &lead[lead.trim_end().len()..];
            let colon = &text[last.colon.clone()];
            let at = last.value.end;
            format!("{},{lead}{key}{colon}{value}{}", &text[..at], &text[at..])
        }
        None if style.lines => {
            let outer = line_indent(text, obj.open);
            format!(
                "{}{{\n{outer}{}{key}{}{value}\n{outer}}}{}",
                &text[..obj.open],
                style.indent,
                style.colon,
                &text[obj.close + 1..]
            )
        }
        None => format!(
            "{}{{{key}{}{value}}}{}",
            &text[..obj.open],
            style.colon,
            &text[obj.close + 1..]
        ),
    }
}

/// `text` without member `i` of `obj`, and the comma that went with it.
fn remove(text: &str, obj: &Object, i: usize) -> String {
    let m = &obj.members[i];
    let span = match (i.checked_sub(1), obj.members.get(i + 1)) {
        (Some(prev), _) => obj.members[prev].value.end..m.value.end,
        (None, Some(next)) => m.start..next.start,
        (None, None) => obj.open + 1..obj.close,
    };
    format!("{}{}", &text[..span.start], &text[span.end..])
}

/// Codex's `config.toml`, parsed to be changed: refused when it does not
/// parse.
pub(super) fn toml_doc(text: &str) -> Result<DocumentMut, String> {
    text.parse::<DocumentMut>().map_err(|e| {
        let why = e.to_string();
        let first = why
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim()
            .trim_start_matches("TOML parse error ")
            .to_string();
        format!("is not valid TOML ({first})")
    })
}

/// The `[[skills.config]]` entries of `doc`, made when there are none.
fn skill_entries(doc: &mut DocumentMut) -> Result<&mut ArrayOfTables, String> {
    let skills = doc.entry("skills").or_insert_with(|| {
        let mut t = Table::new();
        t.set_implicit(true);
        Item::Table(t)
    });
    let Some(skills) = skills.as_table_mut() else {
        return Err("has a `skills` the page does not edit: not a `[skills]` table".into());
    };
    let config = skills
        .entry("config")
        .or_insert_with(|| Item::ArrayOfTables(ArrayOfTables::new()));
    config.as_array_of_tables_mut().ok_or_else(|| {
        "has a `skills.config` the page does not edit: not `[[skills.config]]` tables".into()
    })
}

/// Make Codex load the skill at `paths` (its `SKILL.md`s), named `name`, or
/// not: each entry already naming one of its paths says `enabled = on`,
/// and when an entry after them still says otherwise, one more for each
/// path does, last. Every other entry, key and comment is kept.
pub(super) fn codex_set(
    doc: &mut DocumentMut,
    paths: &[PathBuf],
    name: &str,
    on: bool,
) -> Result<(), String> {
    let entries = skill_entries(doc)?;
    let path_of = |t: &Table| t.get("path").and_then(|v| v.as_str()).map(PathBuf::from);
    for path in paths {
        let last = entries
            .iter_mut()
            .filter(|t| t.get("name").is_none() && path_of(t).as_deref() == Some(path.as_path()))
            .last();
        if let Some(entry) = last {
            set_bool(entry, "enabled", on);
        }
    }
    if codex_enabled(entries, paths, name) != on {
        for path in paths {
            let mut entry = Table::new();
            entry["path"] = toml_edit::value(path.display().to_string());
            entry["enabled"] = toml_edit::value(on);
            entries.push(entry);
        }
    }
    if entries.is_empty() {
        // Nothing was there, and nothing had to be: leave no empty table.
        let skills = doc["skills"].as_table_mut().expect("made above");
        skills.remove("config");
        if skills.is_empty() && skills.is_implicit() {
            doc.remove("skills");
        }
    }
    Ok(())
}

/// Whether Codex loads the skill by `entries`, as `CodexConfig::enabled`
/// reads them.
pub(super) fn codex_enabled(entries: &ArrayOfTables, paths: &[PathBuf], name: &str) -> bool {
    entries
        .iter()
        .filter(|t| {
            let path = t.get("path").and_then(|v| v.as_str());
            let named = t.get("name").and_then(|v| v.as_str());
            match (path, named) {
                (Some(path), None) => paths.iter().any(|p| p == Path::new(path)),
                (None, Some(n)) => !n.is_empty() && n == name,
                _ => false,
            }
        })
        .last()
        .is_none_or(|t| t.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true))
}

/// What `doc` says of the skill, before a change.
pub(super) fn codex_was(doc: &DocumentMut, paths: &[PathBuf], name: &str) -> bool {
    doc.get("skills")
        .and_then(|s| s.get("config"))
        .and_then(Item::as_array_of_tables)
        .is_none_or(|entries| codex_enabled(entries, paths, name))
}

/// `new`, when it parses and is `old` with nothing changed but the
/// `[[skills.config]]` entries for the skill at `paths`: each one that
/// selects it by one of those paths may say `enabled = on` where it said
/// otherwise, entries of just those paths and `enabled = on` may follow
/// the rest, and with them Codex must load the skill or not, as `on` says.
pub(super) fn codex_checked(
    old: &str,
    new: String,
    paths: &[PathBuf],
    name: &str,
    on: bool,
) -> Result<String, Unedited> {
    let unsafe_edit = || Unedited::Unsafe("`skills.config`".into());
    let split = |text: &str| -> Option<(toml::Table, Vec<toml::Value>)> {
        let mut t: toml::Table = toml::from_str(text).ok()?;
        let mut entries = Vec::new();
        if let Some(toml::Value::Table(skills)) = t.get_mut("skills") {
            match skills.remove("config") {
                Some(toml::Value::Array(a)) => entries = a,
                Some(_) => return None,
                None => {}
            }
            if skills.is_empty() {
                t.remove("skills");
            }
        }
        Some((t, entries))
    };
    let (Some((old_rest, old_entries)), Some((new_rest, new_entries))) = (split(old), split(&new))
    else {
        return Err(unsafe_edit());
    };
    let path_of = |t: &toml::Table| {
        t.get("path")
            .and_then(|v| v.as_str())
            .filter(|p| paths.iter().any(|x| x == Path::new(p)))
            .is_some()
    };
    let flipped = |o: &toml::Value, n: &toml::Value| {
        let (Some(o), Some(n)) = (o.as_table(), n.as_table()) else {
            return false;
        };
        let rest = |t: &toml::Table| {
            let mut t = t.clone();
            t.remove("enabled");
            t
        };
        o == n
            || (path_of(o)
                && !o.contains_key("name")
                && rest(o) == rest(n)
                && n.get("enabled") == Some(&toml::Value::Boolean(on)))
    };
    let added = |n: &toml::Value| {
        n.as_table().is_some_and(|t| {
            t.len() == 2 && path_of(t) && t.get("enabled") == Some(&toml::Value::Boolean(on))
        })
    };
    let enabled = new_entries
        .iter()
        .filter_map(|e| e.as_table())
        .rfind(|t| {
            match (
                t.get("path").and_then(|v| v.as_str()),
                t.get("name").and_then(|v| v.as_str()),
            ) {
                (Some(_), None) => path_of(t),
                (None, Some(n)) => !n.is_empty() && n == name,
                _ => false,
            }
        })
        .is_none_or(|t| t.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true));
    let held = old_rest == new_rest
        && new_entries.len() >= old_entries.len()
        && old_entries
            .iter()
            .zip(&new_entries)
            .all(|(o, n)| flipped(o, n))
        && new_entries[old_entries.len()..].iter().all(added)
        && enabled == on;
    if held {
        Ok(new)
    } else {
        Err(unsafe_edit())
    }
}

/// `table.key = on`, the value's own spacing and comment kept.
fn set_bool(table: &mut Table, key: &str, on: bool) {
    match table.get_mut(key).and_then(Item::as_value_mut) {
        Some(v) if v.as_bool() == Some(on) => {}
        Some(v) => {
            let decor = v.decor().clone();
            *v = on.into();
            *v.decor_mut() = decor;
        }
        None => table[key] = toml_edit::value(on),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(text: &str, key: &str, value: Option<&str>) -> String {
        json_set(text, "skillOverrides", key, value).unwrap()
    }

    #[test]
    fn a_key_is_set_where_it_is_and_nothing_else_moves() {
        let text = "{\n    \"model\": \"x\",\n    \"skillOverrides\": {\n        \"a\": \"off\"\n    },\n    \"zeta\": [1,2]\n}\n";
        assert_eq!(
            set(text, "a", Some("\"name-only\"")),
            text.replace("\"a\": \"off\"", "\"a\": \"name-only\"")
        );
        assert_eq!(
            set(text, "b", Some("\"off\"")),
            text.replace("\"a\": \"off\"", "\"a\": \"off\",\n        \"b\": \"off\""),
            "added after the last, laid out as it is"
        );
        assert_eq!(
            set(text, "a", None),
            text.replace("\n        \"a\": \"off\"\n    ", "")
        );
        assert_eq!(set(text, "nope", None), text, "nothing to remove");
    }

    #[test]
    fn a_missing_object_is_added_in_the_files_own_indent() {
        let text = "{\n\t\"model\": \"x\"\n}";
        assert_eq!(
            set(text, "a", Some("\"off\"")),
            "{\n\t\"model\": \"x\",\n\t\"skillOverrides\": {\n\t\t\"a\": \"off\"\n\t}\n}"
        );
        assert_eq!(
            set("{\"model\":\"x\"}", "a", Some("\"off\"")),
            "{\"model\":\"x\",\"skillOverrides\":{\"a\":\"off\"}}",
            "a file on one line stays on one"
        );
        assert_eq!(
            set("", "a", Some("\"off\"")),
            "{\n  \"skillOverrides\": {\n    \"a\": \"off\"\n  }\n}\n"
        );
        assert_eq!(set("", "a", None), "", "no file for nothing");
        assert_eq!(
            set("{\n  \"skillOverrides\": {}\n}\n", "a", Some("\"off\"")),
            "{\n  \"skillOverrides\": {\n    \"a\": \"off\"\n  }\n}\n"
        );
    }

    #[test]
    fn the_first_or_only_member_goes_with_its_comma() {
        let text = r#"{"skillOverrides": {"a": "off", "b": "on"}}"#;
        assert_eq!(set(text, "a", None), r#"{"skillOverrides": {"b": "on"}}"#);
        assert_eq!(set(text, "b", None), r#"{"skillOverrides": {"a": "off"}}"#);
        assert_eq!(
            set(r#"{"skillOverrides": {"a": "off"}}"#, "a", None),
            r#"{"skillOverrides": {}}"#
        );
        assert_eq!(
            set(r#"{"skillOverrides": {"a": "x", "a": "y"}}"#, "a", None),
            r#"{"skillOverrides": {}}"#,
            "every entry of the name"
        );
    }

    #[test]
    fn strings_with_braces_and_escapes_are_skipped_whole() {
        let text = r#"{"x": "}{\"", "skillOverrides": {"we\"ird": "off"}, "y": {"z": "]"}}"#;
        assert_eq!(
            set(text, "we\"ird", Some("\"on\"")),
            text.replace(r#""we\"ird": "off""#, r#""we\"ird": "on""#)
        );
    }

    #[test]
    fn what_does_not_parse_or_is_not_an_object_is_refused() {
        assert!(json_set("{\"a\": ", "skillOverrides", "a", Some("\"off\""))
            .unwrap_err()
            .starts_with("is not valid JSON"));
        assert_eq!(
            json_set("[]", "skillOverrides", "a", Some("\"off\"")).unwrap_err(),
            "does not hold a JSON object"
        );
        assert_eq!(
            json_set(
                "{\"skillOverrides\": null}",
                "skillOverrides",
                "a",
                Some("\"off\"")
            )
            .unwrap_err(),
            "has a `skillOverrides` that is not a JSON object"
        );
    }

    #[test]
    fn a_codex_entry_is_flipped_in_place_or_added_last() {
        let md = PathBuf::from("/h/.agents/skills/a/SKILL.md");
        let text = "# mine\nmodel = \"x\" # the model\n\n[[skills.config]]\npath = \"/h/.agents/skills/a/SKILL.md\"\nenabled = true # was off\n\n[tui]\ntheme = \"dark\"\n";
        let mut doc = toml_doc(text).unwrap();
        codex_set(&mut doc, std::slice::from_ref(&md), "a", false).unwrap();
        assert_eq!(
            doc.to_string(),
            text.replace("enabled = true # was off", "enabled = false # was off")
        );

        let text = "model = \"x\"\n\n# names\n[[skills.config]]\nname = \"a\"\nenabled = false\n";
        let mut doc = toml_doc(text).unwrap();
        assert!(!codex_was(&doc, std::slice::from_ref(&md), "a"));
        codex_set(&mut doc, std::slice::from_ref(&md), "a", true).unwrap();
        assert_eq!(
            doc.to_string(),
            format!(
                "{text}\n[[skills.config]]\npath = \"{}\"\nenabled = true\n",
                md.display()
            ),
            "a later entry by path wins over the one by name"
        );

        let mut doc = toml_doc("model = \"x\"\n").unwrap();
        codex_set(&mut doc, std::slice::from_ref(&md), "a", true).unwrap();
        assert_eq!(doc.to_string(), "model = \"x\"\n", "on already: nothing");
        codex_set(&mut doc, std::slice::from_ref(&md), "a", false).unwrap();
        assert_eq!(
            doc.to_string(),
            format!(
                "model = \"x\"\n\n[[skills.config]]\npath = \"{}\"\nenabled = false\n",
                md.display()
            )
        );
    }

    /// What a splice makes is parsed again and held to the one change:
    /// another key changed, the wrong value, or text that does not parse
    /// is refused, whatever made it.
    #[test]
    fn a_json_edit_that_says_more_than_its_change_is_refused() {
        let old = "{\n  \"model\": \"x\",\n  \"skillOverrides\": {\"b\": \"off\"}\n}\n";
        let check = |new: &str, value: Option<&str>| {
            json_checked(old, new.to_string(), "skillOverrides", "a", value)
        };
        let unsafe_edit = Err(Unedited::Unsafe("`skillOverrides.a`".into()));
        let right = json_set(old, "skillOverrides", "a", Some("\"off\"")).unwrap();
        assert_eq!(check(&right, Some("\"off\"")), Ok(right.clone()));
        assert_eq!(
            check(&right.replace("\"x\"", "\"y\""), Some("\"off\"")),
            unsafe_edit,
            "another key changed"
        );
        assert_eq!(
            check(&right, Some("\"on\"")),
            unsafe_edit,
            "the wrong value"
        );
        assert_eq!(
            check(&right.replace("\"b\": \"off\",", ""), Some("\"off\"")),
            unsafe_edit,
            "an entry lost"
        );
        assert_eq!(
            check("{\"model\": ", Some("\"off\"")),
            unsafe_edit,
            "it does not parse"
        );
        assert_eq!(check("", Some("\"off\"")), unsafe_edit, "it was emptied");
        let gone = json_set(old, "skillOverrides", "b", None).unwrap();
        assert_eq!(
            json_checked(old, gone.clone(), "skillOverrides", "b", None),
            Ok(gone),
            "an emptied object is allowed"
        );
        assert_eq!(
            json_edit("", "skillOverrides", "a", Some("\"off\"")),
            Ok("{\n  \"skillOverrides\": {\n    \"a\": \"off\"\n  }\n}\n".into()),
            "an object made for the key is allowed"
        );
    }

    #[test]
    fn a_codex_edit_that_changes_another_entry_is_refused() {
        let md = PathBuf::from("/h/.agents/skills/a/SKILL.md");
        let paths = std::slice::from_ref(&md);
        let old = "model = \"x\"\n\n[[skills.config]]\nname = \"other\"\nenabled = false\n\n[[skills.config]]\npath = \"/h/.agents/skills/a/SKILL.md\"\nenabled = true\n";
        let mut doc = toml_doc(old).unwrap();
        codex_set(&mut doc, paths, "a", false).unwrap();
        let right = doc.to_string();
        let check = |new: &str| codex_checked(old, new.to_string(), paths, "a", false);
        let unsafe_edit = Err(Unedited::Unsafe("`skills.config`".into()));
        assert_eq!(check(&right), Ok(right.clone()));
        assert_eq!(
            check(&right.replace("model = \"x\"", "model = \"y\"")),
            unsafe_edit,
            "another key"
        );
        assert_eq!(
            check(&right.replacen("enabled = false", "enabled = true", 1)),
            unsafe_edit,
            "another skill's entry"
        );
        assert_eq!(
            check(&format!(
                "{right}\n[[skills.config]]\npath = \"/h/other/SKILL.md\"\nenabled = false\n"
            )),
            unsafe_edit,
            "an entry for another path"
        );
        assert_eq!(
            check(&right.replace(
                "enabled = false\n\n[[skills.config]]\npath",
                "enabled = false\n\n[[skills.config]]\nname = \"a\"\npath"
            )),
            unsafe_edit,
            "an entry that selects by name too"
        );
        assert_eq!(check("model = "), unsafe_edit, "it does not parse");
        assert_eq!(
            codex_checked(old, old.to_string(), paths, "a", false),
            unsafe_edit,
            "it leaves the skill on"
        );
    }

    #[test]
    fn a_codex_file_it_cannot_edit_is_refused() {
        assert!(toml_doc("model = ")
            .unwrap_err()
            .starts_with("is not valid TOML"));
        let md = [PathBuf::from("/a/SKILL.md")];
        let mut doc = toml_doc("skills = 3\n").unwrap();
        assert!(codex_set(&mut doc, &md, "a", false).is_err());
        let mut doc = toml_doc("[skills]\nconfig = [{ name = \"a\", enabled = false }]\n").unwrap();
        assert!(codex_set(&mut doc, &md, "a", true).is_err());
    }
}
