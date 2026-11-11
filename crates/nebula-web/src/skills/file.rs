//! A skill's own file and folder: the one way to the file the page shows,
//! its frontmatter, and the files beside it.

use super::*;
use serde::de::{Deserializer, MapAccess, Visitor};
use std::io::Read;

/// The most of a skill's file that is read.
pub(super) const FILE_MAX: u64 = 1 << 20;
/// The most files a skill's detail lists.
const FILES_MAX: usize = 500;

/// A file's frontmatter fields, in the file's order.
pub(super) type Fields = Vec<(String, serde_json::Value)>;

/// Why a skill's file was not read, and what to call that.
pub(super) type Unread = (api::SkillProblemKind, String);

/// The file a skill is: its real folder's `SKILL.md`, refused when that
/// is a link out of the folder; a command's own real file. The one way to
/// a file the page shows, so nothing outside a skill is ever read.
pub(super) fn skill_file(real: &Path, command: bool) -> Result<PathBuf, Unread> {
    use api::SkillProblemKind as P;
    if command {
        return Ok(real.to_path_buf());
    }
    let link = real.join("SKILL.md");
    let file = fs::canonicalize(&link).map_err(|e| match fs::read_link(&link) {
        Ok(target) => (
            P::BrokenLink,
            format!(
                "Its SKILL.md links to {}, which is not there.",
                target.display()
            ),
        ),
        Err(_) => (P::Unreadable, format!("Its SKILL.md cannot be read: {e}.")),
    })?;
    if !file.starts_with(real) {
        return Err((
            P::LinkOutside,
            format!(
                "Its SKILL.md is a link out of its folder, to {}, so it is not read.",
                file.display()
            ),
        ));
    }
    Ok(file)
}

pub(super) fn read_text(file: &Path) -> Result<String, Unread> {
    let unread = |why: String| (api::SkillProblemKind::Unreadable, why);
    let mut bytes = Vec::new();
    fs::File::open(file)
        .and_then(|f| f.take(FILE_MAX).read_to_end(&mut bytes))
        .map_err(|e| unread(format!("{} cannot be read: {e}.", file.display())))?;
    String::from_utf8(bytes).map_err(|_| unread(format!("{} is not UTF-8 text.", file.display())))
}

/// A file's frontmatter, field by field, and what follows it. Claude Code
/// reads one only when `---` is the file's first line; without one, the
/// whole file is the body.
pub(super) fn frontmatter(text: &str) -> (Result<Fields, String>, &str) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
    if first.trim_end() != "---" {
        return (Ok(Vec::new()), text);
    }
    let mut at = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return (fields(&rest[..at]), &rest[at + line.len()..]);
        }
        at += line.len();
    }
    (
        Err("The frontmatter has no closing `---` line.".into()),
        text,
    )
}

pub(super) fn fields(yaml: &str) -> Result<Fields, String> {
    if yaml.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_saphyr::from_str::<Mapping>(yaml)
        .map(|m| m.0)
        .map_err(|e| {
            let why = e.to_string();
            let why = why
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("")
                .trim();
            crate::sentence(&format!("The frontmatter is not valid YAML: {why}"))
        })
}

/// A YAML mapping's entries in the file's order.
struct Mapping(Fields);

impl<'de> Deserialize<'de> for Mapping {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Entries;
        impl<'de> Visitor<'de> for Entries {
            type Value = Mapping;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("`key: value` fields")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Mapping, A::Error> {
                let mut out = Vec::new();
                while let Some(entry) = map.next_entry::<String, serde_json::Value>()? {
                    out.push(entry);
                }
                Ok(Mapping(out))
            }
        }
        d.deserialize_map(Entries)
    }
}

pub(super) fn field<'a>(fields: &'a [(String, serde_json::Value)], key: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

/// The spec's `name`: lowercase letters, digits and single hyphens inside.
pub(super) fn valid_name(name: &str) -> bool {
    name.len() <= NAME_MAX
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub(super) fn file_entry(rel: PathBuf, path: &Path) -> api::SkillFile {
    let meta = path.symlink_metadata().ok();
    let link = meta
        .as_ref()
        .filter(|m| m.file_type().is_symlink())
        .and_then(|_| fs::read_link(path).ok())
        .map(|t| t.display().to_string());
    api::SkillFile {
        path: rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
        size: if link.is_some() {
            0
        } else {
            meta.map_or(0, |m| m.len())
        },
        link,
    }
}

/// Every file under `dir`, by path, with no link followed: at most
/// [`FILES_MAX`], and whether there were more. A `.git` or `node_modules`
/// is not walked.
pub(super) fn files_in(dir: &Path) -> (Vec<api::SkillFile>, bool) {
    let mut out = Vec::new();
    let mut truncated = false;
    let mut todo = vec![PathBuf::new()];
    while let Some(rel) = todo.pop() {
        let Ok(entries) = fs::read_dir(dir.join(&rel)) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let rel = rel.join(&name);
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if name != ".git" && name != "node_modules" && rel.components().count() < 8 {
                    todo.push(rel);
                }
            } else if out.len() < FILES_MAX {
                out.push(file_entry(rel, &entry.path()));
            } else {
                truncated = true;
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    (out, truncated)
}
