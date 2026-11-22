//! The rules files in the configuration folder (`docs/spec/references.md`, REF-13; D41).
//!
//! `references/` holds one JSON file per set of rules. `references.json` beside it says
//! which are on, the documents each is for, and their order; it is the program's, the rules
//! files' bytes are the user's. Nothing here looks beside or above a document. Reading
//! writes nothing, and each change reads the list again first, so two windows keep each
//! other's changes.

use std::fs;
use std::io::{self, Write as _};
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::references::Rules;

/// The folder of rules files, in the configuration folder.
const FOLDER: &str = "references";
/// The list of their switches, patterns and order, beside it.
const LIST: &str = "references.json";
const EXTENSION: &str = "json";
/// A file larger than this is not rules.
pub const MAX_RULES_BYTES: u64 = 4 * 1024 * 1024;
/// What a new file holds: the empty frame of REF-1.
const NEW_RULES: &str = "{\n  \"includes\": [],\n  \"contexts\": {}\n}\n";
/// Characters a file name cannot have on one of the platforms.
const NOT_IN_NAMES: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

#[derive(Debug, thiserror::Error)]
pub enum FolderError {
    #[error("{path}: {error}")]
    Io { path: PathBuf, error: io::Error },
    #[error("{path} cannot be read, so it is left as it is: {reason}")]
    List { path: PathBuf, reason: String },
    #[error("{0:?} cannot be a file name")]
    Name(String),
    #[error("there is already a file {0}")]
    Exists(String),
    #[error("there is no rules file {0}")]
    Missing(String),
    #[error("{0} is too large to be a rules file")]
    TooLarge(PathBuf),
}

/// The folder and its list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RulesFolder {
    files: PathBuf,
    list: PathBuf,
}

/// One rules file, as it was when the folder was read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Its name in the folder.
    pub name: String,
    pub path: PathBuf,
    pub on: bool,
    /// The documents it is for: patterns separated by `;` ([`applies`]).
    pub use_for: String,
    /// The rules, or why they cannot be used.
    pub rules: Result<Arc<Rules>, String>,
}

/// The folder as it was read, in the list's order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Library {
    pub entries: Vec<Entry>,
    /// Why the folder or its list cannot be read; then no file is on.
    pub error: Option<String>,
}

/// A line of the list.
#[derive(Clone, Debug)]
struct Listed {
    name: String,
    on: bool,
    use_for: String,
}

impl RulesFolder {
    /// The folder in `config`, the folder of the settings file.
    pub fn new(config: &Path) -> Self {
        let config = if config.as_os_str().is_empty() {
            Path::new(".")
        } else {
            config
        };
        Self {
            files: config.join(FOLDER),
            list: config.join(LIST),
        }
    }

    pub fn path(&self) -> &Path {
        &self.files
    }

    /// What to look at to notice a change, besides each entry's file.
    pub fn watched(&self) -> [&Path; 2] {
        [&self.files, &self.list]
    }

    /// Creates the folder, for showing it.
    pub fn create_folder(&self) -> Result<&Path, FolderError> {
        fs::create_dir_all(&self.files).map_err(io_at(&self.files))?;
        Ok(&self.files)
    }

    /// Reads the list and every file. Never fails: what is wrong is in the result.
    pub fn load(&self) -> Library {
        let names = match self.names() {
            Ok(names) => names,
            Err(e) => {
                return Library {
                    entries: Vec::new(),
                    error: Some(e.to_string()),
                };
            }
        };
        let (listed, error) = match self.read_list() {
            Ok(listed) => (listed, None),
            Err(e) => (Vec::new(), Some(e.to_string())),
        };
        let entries = ordered(listed, &names)
            .into_iter()
            .filter(|l| names.contains(&l.name))
            .map(|l| {
                let path = self.files.join(&l.name);
                Entry {
                    rules: read_rules(&path),
                    path,
                    name: l.name,
                    on: l.on,
                    use_for: l.use_for,
                }
            })
            .collect();
        Library { entries, error }
    }

    pub fn set_on(&self, name: &str, on: bool) -> Result<(), FolderError> {
        self.change(name, |list, i| list[i].on = on)
    }

    pub fn set_use_for(&self, name: &str, use_for: &str) -> Result<(), FolderError> {
        self.change(name, |list, i| list[i].use_for = use_for.trim().to_owned())
    }

    /// Moves a file before or after its neighbour; of files for one document the upper wins.
    pub fn move_by(&self, name: &str, up: bool) -> Result<(), FolderError> {
        let names = self.names()?;
        self.change(name, |list, i| {
            // Past lines of files that are not there: they are not shown.
            let shown = |j: &usize| names.contains(&list[*j].name);
            let other = if up {
                (0..i).rev().find(shown)
            } else {
                (i + 1..list.len()).find(shown)
            };
            if let Some(j) = other {
                list.swap(i, j);
            }
        })
    }

    /// Makes a file with an empty frame of rules, off; returns its name.
    pub fn create(&self) -> Result<String, FolderError> {
        self.add("rules", NEW_RULES.as_bytes(), None)
    }

    /// Copies `file` into the folder byte for byte, off; returns the copy's name. The file
    /// it came from is left as it is.
    pub fn import(&self, file: &Path) -> Result<String, FolderError> {
        let bytes = read_small(file)?;
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("rules");
        self.add(stem, &bytes, None)
    }

    /// Copies a file of the folder, off, below it and for the same documents.
    pub fn duplicate(&self, name: &str) -> Result<String, FolderError> {
        let from = self.file(name)?;
        let bytes = read_small(&from)?;
        let stem = Path::new(name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("rules");
        self.add(stem, &bytes, Some(name))
    }

    /// Renames a file and returns its new name; another file is never replaced.
    pub fn rename(&self, name: &str, to: &str) -> Result<String, FolderError> {
        let from = self.file(name)?;
        let to = to.trim();
        let to = if has_extension(to) {
            to.to_owned()
        } else {
            format!("{to}.{EXTENSION}")
        };
        check_name(&to)?;
        if to == name {
            return Ok(to);
        }
        // The list first: one that cannot be read must not lose the file's line.
        let mut list = self.listed()?;
        let target = self.files.join(&to);
        // Only the case changing is the same file where case does not count.
        let taken = fs::symlink_metadata(&target).is_ok()
            && !(name.to_lowercase() == to.to_lowercase() && is_same(&from, &target));
        if taken {
            return Err(FolderError::Exists(to));
        }
        fs::rename(&from, &target).map_err(io_at(&from))?;
        list.retain(|l| l.name != to);
        for line in list.iter_mut().filter(|l| l.name == name) {
            line.name.clone_from(&to);
        }
        self.write_list(&list)?;
        Ok(to)
    }

    /// Deletes a file of the folder and its line.
    pub fn delete(&self, name: &str) -> Result<(), FolderError> {
        let path = self.file(name)?;
        let mut list = self.listed()?;
        fs::remove_file(&path).map_err(io_at(&path))?;
        list.retain(|l| l.name != name);
        self.write_list(&list)
    }

    fn add(&self, stem: &str, bytes: &[u8], below: Option<&str>) -> Result<String, FolderError> {
        let mut list = self.listed()?;
        fs::create_dir_all(&self.files).map_err(io_at(&self.files))?;
        for n in 1.. {
            let name = if n == 1 {
                format!("{stem}.{EXTENSION}")
            } else {
                format!("{stem} ({n}).{EXTENSION}")
            };
            check_name(&name)?;
            let path = self.files.join(&name);
            let mut file = match fs::File::create_new(&path) {
                Ok(file) => file,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(io_at(&path)(e)),
            };
            file.write_all(bytes).map_err(io_at(&path))?;
            // A line left from a file of this name that is gone says nothing about this one.
            list.retain(|l| l.name != name);
            let from = below.and_then(|b| list.iter().position(|l| l.name == b));
            let line = Listed {
                name: name.clone(),
                on: false,
                use_for: from.map_or_else(String::new, |i| list[i].use_for.clone()),
            };
            list.insert(from.map_or(list.len(), |i| i + 1), line);
            self.write_list(&list)?;
            return Ok(name);
        }
        Err(FolderError::Exists(stem.to_owned()))
    }

    fn change(
        &self,
        name: &str,
        change: impl FnOnce(&mut Vec<Listed>, usize),
    ) -> Result<(), FolderError> {
        self.file(name)?;
        let mut list = self.listed()?;
        let i = list
            .iter()
            .position(|l| l.name == name)
            .ok_or_else(|| FolderError::Missing(name.to_owned()))?;
        change(&mut list, i);
        self.write_list(&list)
    }

    /// The path of `name`, which must be a file of the folder.
    fn file(&self, name: &str) -> Result<PathBuf, FolderError> {
        if self.names()?.iter().any(|n| n == name) {
            Ok(self.files.join(name))
        } else {
            Err(FolderError::Missing(name.to_owned()))
        }
    }

    /// The JSON files of the folder, by name; none when there is no folder.
    fn names(&self) -> Result<Vec<String>, FolderError> {
        let dir = match fs::read_dir(&self.files) {
            Ok(dir) => dir,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io_at(&self.files)(e)),
        };
        let mut names: Vec<String> = dir
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_file())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| has_extension(name) && !name.starts_with('.'))
            .collect();
        names.sort_by_key(|name| name.to_lowercase());
        Ok(names)
    }

    /// The list with a line for every file there is.
    fn listed(&self) -> Result<Vec<Listed>, FolderError> {
        Ok(ordered(self.read_list()?, &self.names()?))
    }

    fn read_list(&self) -> Result<Vec<Listed>, FolderError> {
        let bytes = match fs::read(&self.list) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io_at(&self.list)(e)),
        };
        let bad = |reason: String| FolderError::List {
            path: self.list.clone(),
            reason,
        };
        let value: Value = serde_json::from_slice(&bytes).map_err(|e| bad(e.to_string()))?;
        let files = value
            .get("files")
            .and_then(Value::as_array)
            .ok_or_else(|| bad("it has no list \"files\"".into()))?;
        files
            .iter()
            .map(|file| {
                Some(Listed {
                    name: file.get("name")?.as_str()?.to_owned(),
                    on: file.get("on")?.as_bool()?,
                    use_for: file.get("use_for")?.as_str()?.to_owned(),
                })
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| bad("a file in it lacks name, on or use_for".into()))
    }

    fn write_list(&self, list: &[Listed]) -> Result<(), FolderError> {
        let files: Vec<Value> = list
            .iter()
            .map(|l| json!({"name": l.name, "on": l.on, "use_for": l.use_for}))
            .collect();
        if let Some(dir) = self.list.parent() {
            fs::create_dir_all(dir).map_err(io_at(dir))?;
        }
        let text = format!("{:#}\n", json!({ "files": files }));
        crate::save_atomic(&self.list, text.as_bytes()).map_err(io_at(&self.list))
    }
}

impl Library {
    /// The file used for `document`: the upper of those that are on and apply to it. Also
    /// one that cannot be read, so that the window says so and does not take the next.
    pub fn for_document(&self, document: Option<&Path>) -> Option<&Entry> {
        let resolved = document.map(resolved);
        self.entries
            .iter()
            .find(|e| e.on && applies(&e.use_for, resolved.as_deref()))
    }
}

/// Whether `use_for` names the document at `document`, a resolved path; `None` for one
/// without a file yet, which only `*` names. `*` stands for any characters, folders included,
/// and `?` for one. A pattern without `/` is compared with the file's name, one with `/` with
/// the end of the path from a folder's start.
pub fn applies(use_for: &str, document: Option<&Path>) -> bool {
    let mut patterns = use_for.split(';').map(str::trim).filter(|p| !p.is_empty());
    let Some(path) = document else {
        return patterns.any(|p| p == "*");
    };
    let path = comparable(&path.to_string_lossy());
    patterns.any(|pattern| {
        let pattern: Vec<char> = comparable(pattern);
        if !pattern.contains(&'/') {
            let name = path.iter().rposition(|&c| c == '/').map_or(0, |i| i + 1);
            return glob(&pattern, &path[name..]);
        }
        let whole = glob(&pattern, &path);
        // From a folder's start, unless the pattern says where the path starts.
        whole
            || (pattern[0] != '/'
                && pattern[0] != '*'
                && (0..path.len()).any(|i| path[i] == '/' && glob(&pattern, &path[i + 1..])))
    })
}

/// Where `path` really is: absolute, without links or `..`. For a file that is not there,
/// where its folder is; if that is not known either, the path made absolute.
pub fn resolved(path: &Path) -> PathBuf {
    if let Ok(real) = fs::canonicalize(path) {
        return without_verbatim(real);
    }
    let folder = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    match (fs::canonicalize(folder), path.file_name()) {
        (Ok(real), Some(name)) => without_verbatim(real).join(name),
        _ => std::path::absolute(path).unwrap_or_else(|_| path.to_owned()),
    }
}

/// The lines of `list` in their order, then one, off, for each file that has none.
fn ordered(mut list: Vec<Listed>, names: &[String]) -> Vec<Listed> {
    let mut seen = std::collections::HashSet::new();
    list.retain(|l| seen.insert(l.name.clone()));
    for name in names {
        if !seen.contains(name) {
            list.push(Listed {
                name: name.clone(),
                on: false,
                use_for: String::new(),
            });
        }
    }
    list
}

fn read_rules(path: &Path) -> Result<Arc<Rules>, String> {
    let bytes = read_small(path).map_err(|e| e.to_string())?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "not UTF-8 text".to_owned())?;
    Rules::parse(text).map(Arc::new).map_err(|e| e.to_string())
}

fn read_small(path: &Path) -> Result<Vec<u8>, FolderError> {
    if fs::metadata(path).map_err(io_at(path))?.len() > MAX_RULES_BYTES {
        return Err(FolderError::TooLarge(path.to_owned()));
    }
    fs::read(path).map_err(io_at(path))
}

fn io_at(path: &Path) -> impl FnOnce(io::Error) -> FolderError + '_ {
    move |error| FolderError::Io {
        path: path.to_owned(),
        error,
    }
}

fn has_extension(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
}

fn check_name(name: &str) -> Result<(), FolderError> {
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    let bad = stem.trim().is_empty()
        || name.starts_with('.')
        || name.contains(NOT_IN_NAMES)
        || name.chars().any(char::is_control);
    if bad {
        return Err(FolderError::Name(name.to_owned()));
    }
    Ok(())
}

fn is_same(a: &Path, b: &Path) -> bool {
    matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(a), Ok(b)) if a == b)
}

/// For comparing: case does not count on Windows and macOS, and Windows has two separators.
fn comparable(text: &str) -> Vec<char> {
    let mut text = text.to_owned();
    if cfg!(windows) {
        text = text.replace('\\', "/");
    }
    if cfg!(any(windows, target_os = "macos")) {
        text = text.to_lowercase();
    }
    text.chars().collect()
}

/// `*` for any characters and `?` for one; in time pattern times text.
fn glob(pattern: &[char], text: &[char]) -> bool {
    let (mut p, mut t) = (0, 0);
    let mut back: Option<(usize, usize)> = None;
    while t < text.len() {
        if pattern.get(p) == Some(&'*') {
            p += 1;
            back = Some((p, t));
        } else if pattern.get(p).is_some_and(|&c| c == '?' || c == text[t]) {
            p += 1;
            t += 1;
        } else if let Some((after_star, from)) = back {
            (p, t) = (after_star, from + 1);
            back = Some((after_star, from + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

/// `C:\dir` for the `\\?\C:\dir` that `canonicalize` gives on Windows: what a pattern is
/// written for.
fn without_verbatim(path: PathBuf) -> PathBuf {
    let mut parts = path.components();
    if let Some(Component::Prefix(prefix)) = parts.next()
        && let Prefix::VerbatimDisk(drive) = prefix.kind()
    {
        parts.next(); // The root after the drive.
        return Path::new(&format!("{}:\\", char::from(drive))).join(parts.as_path());
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: &str =
        "{\r\n  \"includes\": [{\"element\": \"Größe\", \"attribute\": \"src\"}]\r\n}\r\n";

    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("xml-atlas-folder-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn folder(&self) -> RulesFolder {
            RulesFolder::new(&self.0.join("config"))
        }

        fn file(&self, name: &str, text: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, text).unwrap();
            path
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn names(folder: &RulesFolder) -> Vec<String> {
        folder.load().entries.into_iter().map(|e| e.name).collect()
    }

    /// REF-13: only a file that is on and whose Use for names the document applies.
    #[test]
    fn a_file_applies_only_when_on_and_for_the_document() {
        let t = Temp::new("applies");
        let folder = t.folder();
        let from = t.file("elsewhere/Größe.json", RULES);
        let doc = t.file("docs/Größe.xml", "<a/>");
        let name = folder.import(&from).unwrap();

        let used = || {
            folder
                .load()
                .for_document(Some(&doc))
                .map(|e| e.name.clone())
        };
        assert_eq!(used(), None, "imported, it is off");
        folder.set_on(&name, true).unwrap();
        assert_eq!(used(), None, "an empty Use for names nothing");
        folder.set_use_for(&name, "andere.xml").unwrap();
        assert_eq!(used(), None, "another document's");
        folder.set_use_for(&name, "andere.xml; Größe*.xml").unwrap();
        assert_eq!(used(), Some(name.clone()));
        folder.set_on(&name, false).unwrap();
        assert_eq!(used(), None, "switched off");
    }

    /// D41: what lies beside or above the document is not looked at.
    #[test]
    fn a_file_beside_the_document_is_not_used() {
        let t = Temp::new("beside");
        let doc = t.file("docs/sub/catalog.xml", "<a/>");
        t.file("docs/sub/xml-atlas-references.json", RULES);
        t.file("docs/xml-atlas-references.json", RULES);
        t.file("docs/sub/catalog.json", RULES);
        let library = t.folder().load();
        assert_eq!(library, Library::default());
        assert!(library.for_document(Some(&doc)).is_none());
        assert!(!t.0.join("config").exists(), "reading writes nothing");
    }

    #[test]
    fn patterns_name_files_and_the_ends_of_paths() {
        let yes = |pattern: &str, path: &str| applies(pattern, Some(Path::new(path)));
        assert!(yes("catalog.xml", "/w/a/catalog.xml"));
        assert!(!yes("catalog.xml", "/w/a/old-catalog.xml"));
        assert!(yes("cat*.xml", "/w/catalog.xml"));
        assert!(!yes("w*.xml", "/w/catalog.xml"), "a name, not a path");
        assert!(
            yes("Grö?e.xml", "/w/Größe.xml"),
            "? is a character, not a byte"
        );
        assert!(yes("XML/catalog.xml", "/w/XML/catalog.xml"));
        assert!(
            !yes("XML/catalog.xml", "/w/oldXML/catalog.xml"),
            "a folder's start"
        );
        assert!(yes("*/XML/catalog.xml", "/w/build/XML/catalog.xml"));
        assert!(!yes("*/XML/catalog.xml", "/w/XML/North/catalog.xml"));
        assert!(yes("*/XML/*catalog.xml", "/w/XML/North/catalog.xml"));
        assert!(yes("*/XML/*catalog.xml", "/w/XML/catalog.xml"));
        assert!(!yes("*/XML/*catalog.xml", "/w/plugins/catalog.xml"));
        assert!(yes("/w/*.xml", "/w/a/b.xml"));
        assert!(
            !yes("/a/*.xml", "/w/a/b.xml"),
            "a leading / is the path's start"
        );
        assert!(yes("*", "/w/any"));
        assert!(!yes("", "/w/any") && !yes(" ; ", "/w/any"));
        assert!(applies("x.xml; *", None));
        assert!(
            !applies("*.xml", None),
            "only * names a document without a file"
        );
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn case_does_not_count_here() {
        assert!(applies("größe.XML", Some(Path::new("/w/Größe.xml"))));
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths_match_either_separator() {
        let path = without_verbatim(PathBuf::from(r"\\?\C:\Work\XML\Größe.xml"));
        assert_eq!(path.as_os_str(), r"C:\Work\XML\Größe.xml");
        assert!(applies("XML/größe.xml", Some(&path)));
        assert!(applies(r"C:\work\*", Some(&path)));
    }

    /// The path is resolved before it is matched: a folder it only passes through is not
    /// where the document is.
    #[test]
    fn dot_dot_is_resolved_before_matching() {
        let t = Temp::new("dots");
        t.file("passed/keep", "");
        t.file("real/doc.xml", "<a/>");
        let folder = t.folder();
        let name = folder.import(&t.file("r.json", RULES)).unwrap();
        folder.set_on(&name, true).unwrap();
        let given = t.0.join("passed").join("..").join("real").join("doc.xml");
        let used = |pattern: &str| {
            folder.set_use_for(&name, pattern).unwrap();
            folder.load().for_document(Some(&given)).is_some()
        };
        assert!(!used("*/passed/*"));
        assert!(used("real/doc.xml"));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_matched_by_its_target() {
        let t = Temp::new("link");
        let real = t.file("real/doc.xml", "<a/>");
        fs::create_dir_all(t.0.join("links")).unwrap();
        let link = t.0.join("links/alias.xml");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(applies("real/doc.xml", Some(&resolved(&link))));
        assert!(!applies("alias.xml", Some(&resolved(&link))));
    }

    #[test]
    fn the_upper_of_two_wins() {
        let t = Temp::new("order");
        let folder = t.folder();
        let doc = t.file("doc.xml", "<a/>");
        let from = t.file("r.json", RULES);
        let first = folder.import(&from).unwrap();
        let second = folder.import(&from).unwrap();
        for name in [&first, &second] {
            folder.set_on(name, true).unwrap();
            folder.set_use_for(name, "*").unwrap();
        }
        let used = || folder.load().for_document(Some(&doc)).unwrap().name.clone();
        assert_eq!(used(), first);
        folder.move_by(&second, true).unwrap();
        assert_eq!(used(), second);
        assert_eq!(names(&folder), [second, first]);
    }

    /// A broken file is the one in use, with its error: the next one does not step in.
    #[test]
    fn a_broken_file_in_use_says_so() {
        let t = Temp::new("broken");
        let folder = t.folder();
        for text in ["{\"includes\": [],}", RULES] {
            let name = folder.import(&t.file("r.json", text)).unwrap();
            folder.set_on(&name, true).unwrap();
            folder.set_use_for(&name, "*").unwrap();
        }
        let library = folder.load();
        assert!(library.for_document(None).unwrap().rules.is_err());
    }

    #[test]
    fn import_and_duplicate_copy_and_replace_nothing() {
        let t = Temp::new("copies");
        let folder = t.folder();
        let text = format!("\u{feff}{RULES}");
        let from = t.file("Größe.json", &text);
        assert_eq!(folder.import(&from).unwrap(), "Größe.json");
        assert_eq!(folder.import(&from).unwrap(), "Größe (2).json");
        folder.set_on("Größe.json", true).unwrap();
        folder.set_use_for("Größe.json", "g.xml").unwrap();
        assert_eq!(folder.duplicate("Größe.json").unwrap(), "Größe (3).json");

        let entries = folder.load().entries;
        assert_eq!(entries[1].name, "Größe (3).json", "below the one copied");
        assert_eq!(
            (entries[1].on, entries[1].use_for.as_str()),
            (false, "g.xml")
        );
        for e in &entries {
            assert_eq!(fs::read(&e.path).unwrap(), text.as_bytes(), "{}", e.name);
        }
        assert_eq!(
            fs::read(&from).unwrap(),
            text.as_bytes(),
            "the original stays"
        );
    }

    #[test]
    fn rename_keeps_the_line_and_stays_in_the_folder() {
        let t = Temp::new("rename");
        let folder = t.folder();
        let a = folder.create().unwrap();
        let b = folder.create().unwrap();
        folder.set_on(&a, true).unwrap();
        folder.set_use_for(&a, "a.xml").unwrap();
        assert!(matches!(folder.rename(&a, &b), Err(FolderError::Exists(_))));
        for bad in ["", "../x", "a/b", ".hidden", "c:d", "  "] {
            assert!(folder.rename(&a, bad).is_err(), "{bad:?}");
        }
        assert_eq!(folder.rename(&a, "Katalog").unwrap(), "Katalog.json");
        let e = &folder.load().entries[0];
        assert_eq!(
            (e.name.as_str(), e.on, e.use_for.as_str()),
            ("Katalog.json", true, "a.xml")
        );
        assert!(e.rules.is_ok(), "a new file's frame is valid");
        assert!(
            folder.delete("../../r.json").is_err(),
            "only files of the folder"
        );
        assert!(folder.set_on("no.json", true).is_err());
    }

    /// A deleted file's switch does not pass to a new file of the same name.
    #[test]
    fn a_new_file_never_starts_on() {
        let t = Temp::new("again");
        let folder = t.folder();
        let name = folder.create().unwrap();
        folder.set_on(&name, true).unwrap();
        fs::remove_file(folder.path().join(&name)).unwrap();
        assert!(names(&folder).is_empty());
        assert_eq!(folder.create().unwrap(), name);
        assert!(!folder.load().entries[0].on);
        // Put there by hand: listed, and off.
        fs::write(folder.path().join("by-hand.json"), RULES).unwrap();
        fs::write(folder.path().join("notes.txt"), "no").unwrap();
        let entries = folder.load().entries;
        assert_eq!(entries.len(), 2);
        assert!(!entries[1].on);
        folder.delete(&name).unwrap();
        assert_eq!(names(&folder), ["by-hand.json"]);
    }

    /// A list that cannot be read is never written over.
    #[test]
    fn a_damaged_list_is_left_as_it_is() {
        let t = Temp::new("damaged");
        let folder = t.folder();
        let name = folder.create().unwrap();
        let list = t.0.join("config").join(LIST);
        fs::write(&list, "{ half").unwrap();
        let library = folder.load();
        assert!(library.error.is_some());
        assert!(library.entries.iter().all(|e| !e.on));
        assert!(folder.set_on(&name, true).is_err());
        assert!(folder.create().is_err());
        assert!(folder.rename(&name, "x").is_err());
        assert!(folder.delete(&name).is_err());
        assert_eq!(fs::read_to_string(&list).unwrap(), "{ half");
        assert_eq!(names(&folder), [name]);
    }
}
