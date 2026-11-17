//! `cargo xtask package-macos`: the macOS disk image (`docs/spec/install.md`, PKG-20).
//!
//! Runs on macOS. It builds both programs for Apple silicon and Intel, joins each pair into
//! one universal executable with `lipo`, puts them into `XML Atlas.app` with the icon and the
//! license texts, signs the app ad hoc (no identity: the Developer ID signing and
//! notarization of PKG-21 are a manual step for later), and writes
//! `target/dist/xml-atlas-<version>-macos-universal.dmg` with `hdiutil`. The disk image holds
//! the app and a link to Applications, to drag it into.

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, bail};

use crate::package::{BINARIES, NAME, metadata, third_party_licenses, version};

/// The two targets joined into universal executables.
const TARGETS: [&str; 2] = ["aarch64-apple-darwin", "x86_64-apple-darwin"];

/// The oldest macOS the app runs on (PKG-20); the build and `Info.plist` both say so.
const MIN_MACOS: &str = "12.0";

/// The bundle id, the same as the Linux desktop entry's (PKG-32, PKG-33).
const BUNDLE_ID: &str = "io.github.glaure.XmlAtlas";

const APP: &str = "XML Atlas.app";

const ICON: &str = "packaging/macos/xml-atlas.icns";

pub fn package(root: &Path, skip_build: bool) -> anyhow::Result<()> {
    let meta = metadata(root, TARGETS[0])?;
    let version = version(&meta)?;
    if !skip_build {
        for target in TARGETS {
            build(root, target)?;
        }
    }
    let dist = root.join("target/dist");
    let stage = dist.join("macos");
    if stage.exists() {
        fs::remove_dir_all(&stage)?;
    }
    let app = stage.join(APP);
    let contents = app.join("Contents");
    let bin = contents.join("MacOS");
    let resources = contents.join("Resources");
    fs::create_dir_all(&bin)?;
    fs::create_dir_all(&resources)?;

    for name in BINARIES {
        let slices: Vec<_> = TARGETS
            .iter()
            .map(|t| root.join("target").join(t).join("release").join(name))
            .collect();
        run(Command::new("lipo")
            .arg("-create")
            .args(&slices)
            .arg("-output")
            .arg(bin.join(name)))?;
    }
    fs::write(contents.join("Info.plist"), info_plist(&version))?;
    fs::copy(root.join(ICON), resources.join("xml-atlas.icns"))?;
    for name in ["LICENSE-MIT", "LICENSE-APACHE"] {
        fs::copy(root.join(name), resources.join(name))?;
    }
    fs::write(
        resources.join("THIRD-PARTY-LICENSES.txt"),
        third_party_licenses(&meta)?,
    )?;
    // Ad hoc: Apple silicon runs only signed code, and the bundle's parts must agree.
    run(Command::new("codesign")
        .args(["--force", "--deep", "--sign", "-"])
        .arg(&app))?;
    link_applications(&stage.join("Applications"))?;

    let dmg = dist.join(format!("{NAME}-{version}-macos-universal.dmg"));
    run(Command::new("hdiutil")
        .args(["create", "-ov", "-format", "UDZO", "-fs", "HFS+"])
        .args(["-volname", &format!("XML Atlas {version}")])
        .arg("-srcfolder")
        .arg(&stage)
        .arg(&dmg))?;
    println!("wrote {}", dmg.display());
    Ok(())
}

/// The app's `Info.plist`. XML files are declared with the rank Alternate, so the app is
/// listed in *Open with* without becoming the default (PKG-4); it takes them from Finder as
/// `docs/decisions.md`, D26, describes.
pub(crate) fn info_plist(version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>XML Atlas</string>
    <key>CFBundleDisplayName</key>
    <string>XML Atlas</string>
    <key>CFBundleIdentifier</key>
    <string>{BUNDLE_ID}</string>
    <key>CFBundleExecutable</key>
    <string>xml-atlas</string>
    <key>CFBundleIconFile</key>
    <string>xml-atlas</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>{version}</string>
    <key>CFBundleVersion</key>
    <string>{version}</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>LSMinimumSystemVersion</key>
    <string>{MIN_MACOS}</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.developer-tools</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key>
            <string>XML document</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>public.xml</string>
            </array>
        </dict>
    </array>
</dict>
</plist>
"#
    )
}

/// Builds both programs for `target`, stripped, for [`MIN_MACOS`] and later.
fn build(root: &Path, target: &str) -> anyhow::Result<()> {
    let mut cmd = Command::new(env!("CARGO"));
    cmd.current_dir(root)
        .env("MACOSX_DEPLOYMENT_TARGET", MIN_MACOS)
        .args(["build", "--release", "--target", target])
        .args(["--config", "profile.release.strip=\"symbols\""]);
    for bin in BINARIES {
        cmd.args(["-p", bin]);
    }
    run(&mut cmd).with_context(|| format!("building for {target} (rustup target add {target})"))
}

fn run(cmd: &mut Command) -> anyhow::Result<()> {
    let program = cmd.get_program().to_string_lossy().into_owned();
    let status = cmd.status().with_context(|| format!("running {program}"))?;
    if !status.success() {
        bail!("{program} failed");
    }
    Ok(())
}

/// The link to Applications in the disk image, to drag the app onto.
#[cfg(unix)]
fn link_applications(at: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink("/Applications", at)
}

#[cfg(not(unix))]
fn link_applications(_: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("the disk image is made on macOS"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_plist_names_the_app_and_version() {
        let plist = info_plist("1.2.3");
        assert!(plist.contains("<string>io.github.glaure.XmlAtlas</string>"));
        assert!(
            plist.contains("<key>CFBundleShortVersionString</key>\n    <string>1.2.3</string>")
        );
        assert!(plist.contains("<key>CFBundleExecutable</key>\n    <string>xml-atlas</string>"));
        assert!(plist.contains("<string>12.0</string>"));
        // XML files, listed in Open with but not taken as the default (PKG-4).
        assert!(plist.contains("<string>public.xml</string>"));
        assert!(plist.contains("<key>LSHandlerRank</key>\n            <string>Alternate</string>"));
        assert_eq!(
            plist.matches("<dict>").count(),
            plist.matches("</dict>").count()
        );
        assert_eq!(
            plist.matches("<array>").count(),
            plist.matches("</array>").count()
        );
        assert!(plist.trim_end().ends_with("</dict>\n</plist>"));
    }
}
