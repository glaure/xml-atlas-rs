//! Puts the icon into `xml-atlas.exe` on Windows, so Explorer shows it for the program file.
//! On other targets `embed_resource` does nothing.

use embed_resource::CompilationResult;

const RESOURCE: &str = "xml-atlas.rc";
const ICON: &str = "../../packaging/windows/xml-atlas.ico";

fn main() {
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
