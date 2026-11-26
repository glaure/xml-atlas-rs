//! Turns the language files in `lang/` into code (LANG-10), and puts the icon into
//! `xml-atlas.exe` on Windows, so Explorer shows it for the program file. On other targets
//! `embed_resource` does nothing.

use std::path::Path;

use embed_resource::CompilationResult;

#[path = "lang/generate.rs"]
mod generate;

const RESOURCE: &str = "xml-atlas.rc";
const ICON: &str = "../../packaging/windows/xml-atlas.ico";
/// The languages besides English, as their files are named.
const LANGUAGES: [&str; 1] = ["de"];

fn main() {
    texts();
    println!("cargo:rerun-if-changed={RESOURCE}");
    println!("cargo:rerun-if-changed={ICON}");
    match embed_resource::compile_for(RESOURCE, ["xml-atlas"], embed_resource::NONE) {
        CompilationResult::Ok | CompilationResult::NotWindows => {}
        // A check for Windows from another system has no resource compiler; the package
        // (`cargo xtask package-windows`) refuses an executable without the icon.
        CompilationResult::NotAttempted(why) => {
            println!("cargo:warning=no icon in xml-atlas.exe: {why}");
        }
        CompilationResult::Failed(e) => panic!("compiling {RESOURCE}: {e}"),
    }
}

/// Writes `texts.rs` for `src/lang.rs`; warns about texts a language lacks and fails on a
/// wrong one (LANG-12, LANG-13).
fn texts() {
    println!("cargo:rerun-if-changed=lang");
    let read = |code: &str| {
        let path = format!("lang/{code}.toml");
        println!("cargo:rerun-if-changed={path}");
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} cannot be read: {e}"))
    };
    let english = read("en");
    let sources: Vec<(&str, String)> = LANGUAGES.map(|code| (code, read(code))).into();
    let others: Vec<generate::File> = sources
        .iter()
        .map(|(code, source)| generate::File { code, source })
        .collect();
    let english = generate::File {
        code: "en",
        source: &english,
    };
    let generated = generate::generate(&english, &others).unwrap_or_else(|e| panic!("{e}"));
    for warning in generated.warnings {
        println!("cargo:warning={warning}");
    }
    let out = std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR");
    let out = Path::new(&out).join("texts.rs");
    std::fs::write(&out, generated.code).unwrap_or_else(|e| panic!("writing texts.rs: {e}"));
}
