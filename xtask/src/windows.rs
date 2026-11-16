//! `cargo xtask package-windows`: the Windows MSI and zip (`docs/spec/install.md`, PKG-10 to
//! PKG-12).
//!
//! Runs on Windows. It builds both programs with MSVC, collects them with the license texts,
//! and writes `target/dist/xml-atlas-<version>-windows-x64.zip` and, with WiX 5.0.2
//! (`dotnet tool install --global wix --version 5.0.2`,
//! `wix extension add --global WixToolset.UI.wixext/5.0.2`),
//! `target/dist/xml-atlas-<version>-x64.msi` from `packaging/windows/xml-atlas.wxs`. WiX 7
//! does not run until its Open Source Maintenance Fee EULA is accepted, so the version is
//! pinned instead of taking the latest. With
//! `--bin-dir`, it packs executables from elsewhere instead of building: the signing script
//! (PKG-14) packs the signed ones again.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};

use crate::package::{BINARIES, NAME, metadata, third_party_licenses, version};

const TARGET: &str = "x86_64-pc-windows-msvc";

/// The program's icon: in the installer, and in `xml-atlas.exe` through its `build.rs`.
const ICON: &str = "packaging/windows/xml-atlas.ico";

/// The WiX version the MSI is built with; the UI extension must be the same version.
const WIX_VERSION: &str = "5.0.2";

/// What the installer shows on its license page.
const LICENSE_NOTE: &str = "XML Atlas is licensed under the MIT license or the Apache License, \
Version 2.0, at your option. Both texts are installed with it, as LICENSE-MIT and \
LICENSE-APACHE, together with the licenses of the libraries it contains \
(THIRD-PARTY-LICENSES.txt). The MIT license follows.";

pub fn package(
    root: &Path,
    skip_build: bool,
    bin_dir: Option<PathBuf>,
    zip_only: bool,
) -> anyhow::Result<()> {
    let meta = metadata(root, TARGET)?;
    let version = version(&meta)?;
    if !skip_build && bin_dir.is_none() {
        build(root)?;
    }
    let bin_dir = bin_dir.unwrap_or_else(|| root.join("target").join(TARGET).join("release"));
    let dist = root.join("target/dist");
    let stage = dist.join("windows");
    if stage.exists() {
        fs::remove_dir_all(&stage)?;
    }
    fs::create_dir_all(&stage)?;
    let mut files = Vec::new();
    for bin in BINARIES {
        let name = format!("{bin}.exe");
        let from = bin_dir.join(&name);
        fs::copy(&from, stage.join(&name)).with_context(|| from.display().to_string())?;
        files.push(name);
    }
    let exe = fs::read(stage.join("xml-atlas.exe"))?;
    let ico = fs::read(root.join(ICON))?;
    if !has_icon(&exe, &ico) {
        bail!(
            "xml-atlas.exe has no icon: its build needs RC.EXE from the Windows SDK \
             (see the `no icon` warning of cargo build)"
        );
    }
    for name in ["LICENSE-MIT", "LICENSE-APACHE"] {
        fs::copy(root.join(name), stage.join(name))?;
        files.push(name.to_string());
    }
    fs::write(
        stage.join("THIRD-PARTY-LICENSES.txt"),
        crlf(&third_party_licenses(&meta)?),
    )?;
    files.push("THIRD-PARTY-LICENSES.txt".to_string());

    let zip_path = dist.join(format!("{NAME}-{version}-windows-x64.zip"));
    write_zip(&zip_path, &format!("{NAME}-{version}"), &stage, &files)?;
    println!("wrote {}", zip_path.display());
    if zip_only {
        return Ok(());
    }
    let mit = fs::read_to_string(root.join("LICENSE-MIT"))?;
    let rtf_path = stage.join("license.rtf");
    fs::write(&rtf_path, rtf(&format!("{LICENSE_NOTE}\n\n{mit}")))?;
    let msi = dist.join(format!("{NAME}-{version}-x64.msi"));
    msi_build(root, &version, &stage, &rtf_path, &msi)?;
    println!("wrote {}", msi.display());
    Ok(())
}

/// Builds both programs for 64-bit Windows with MSVC.
fn build(root: &Path) -> anyhow::Result<()> {
    let mut cmd = Command::new(env!("CARGO"));
    cmd.current_dir(root)
        .args(["build", "--release", "--target", TARGET]);
    for bin in BINARIES {
        cmd.args(["-p", bin]);
    }
    if !cmd.status().context("running cargo build")?.success() {
        bail!("cargo build failed");
    }
    Ok(())
}

/// Runs WiX on `packaging/windows/xml-atlas.wxs`.
fn msi_build(
    root: &Path,
    version: &str,
    stage: &Path,
    rtf: &Path,
    out: &Path,
) -> anyhow::Result<()> {
    let wxs = root.join("packaging/windows/xml-atlas.wxs");
    let icon = root.join(ICON);
    let define = |name: &str, value: &Path| format!("{name}={}", value.display());
    let status = Command::new("wix")
        .arg("build")
        .arg(&wxs)
        .args(["-arch", "x64", "-ext", "WixToolset.UI.wixext"])
        .args(["-d", &format!("Version={version}")])
        .args(["-d", &define("SourceDir", stage)])
        .args(["-d", &define("IconFile", &icon)])
        .args(["-d", &define("LicenseRtf", rtf)])
        .arg("-o")
        .arg(out)
        .status()
        .with_context(|| {
            format!(
                "running wix: install WiX {WIX_VERSION} with \
                 `dotnet tool install --global wix --version {WIX_VERSION}`, then \
                 `wix extension add --global WixToolset.UI.wixext/{WIX_VERSION}`"
            )
        })?;
    if !status.success() {
        bail!(
            "wix build failed; the MSI is built with WiX {WIX_VERSION} (see `wix --version`; \
             `dotnet tool update --global wix --version {WIX_VERSION} --allow-downgrade`) and \
             its UI extension (`wix extension add --global WixToolset.UI.wixext/{WIX_VERSION}`)"
        );
    }
    Ok(())
}

/// Writes `files` of `stage` into a zip, inside a folder `top`.
fn write_zip(path: &Path, top: &str, stage: &Path, files: &[String]) -> anyhow::Result<()> {
    let mut zip = zip::ZipWriter::new(fs::File::create(path)?);
    let (y, m, d, h, min, sec) = utc_now();
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::from_date_and_time(y, m, d, h, min, sec)?);
    for name in files {
        zip.start_file(format!("{top}/{name}"), options)?;
        zip.write_all(&fs::read(stage.join(name))?)?;
    }
    zip.finish()?;
    Ok(())
}

/// The time now in UTC, as year, month, day, hour, minute and second: the date the zip's
/// files show.
fn utc_now() -> (u16, u8, u8, u8, u8, u8) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    let s = secs % 86_400;
    (
        y as u16,
        m,
        d,
        (s / 3600) as u8,
        (s / 60 % 60) as u8,
        (s % 60) as u8,
    )
}

/// The date `days` after 1970-01-01, in the proleptic Gregorian calendar (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// `text` with Windows line endings, for Notepad.
fn crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

/// `text` as a Rich Text document, as the installer's license page wants it: paragraphs at
/// blank lines, other characters escaped.
fn rtf(text: &str) -> String {
    let mut out = String::from("{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0 Segoe UI;}}\\f0\\fs18 ");
    let text = text.replace("\r\n", "\n");
    for (i, para) in text.split("\n\n").enumerate() {
        if i > 0 {
            out.push_str("\\par\\par ");
        }
        for (j, line) in para.trim_matches('\n').split('\n').enumerate() {
            if j > 0 {
                out.push(' ');
            }
            for c in line.trim().chars() {
                match c {
                    '\\' | '{' | '}' => {
                        out.push('\\');
                        out.push(c);
                    }
                    c if c.is_ascii() => out.push(c),
                    // A 16-bit signed code unit, with `?` for readers without Unicode.
                    c => {
                        let mut units = [0u16; 2];
                        for u in c.encode_utf16(&mut units) {
                            out.push_str(&format!("\\u{}?", *u as i16));
                        }
                    }
                }
            }
        }
    }
    out.push('}');
    out
}

/// Whether every image of the `.ico` file is in the executable. The resource compiler copies
/// each image unchanged into an icon resource; only the directory in front of them changes.
fn has_icon(exe: &[u8], ico: &[u8]) -> bool {
    let u16_at = |at: usize| {
        ico.get(at..at + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    };
    let u32_at = |at: usize| {
        ico.get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    };
    let Some(count) = u16_at(4).filter(|&n| n > 0) else {
        return false;
    };
    (0..usize::from(count)).all(|i| {
        let entry = 6 + 16 * i;
        let image = u32_at(entry + 8)
            .zip(u32_at(entry + 12))
            .and_then(|(len, offset)| ico.get(offset..offset + len));
        image.is_some_and(|image| exe.windows(image.len()).any(|w| w == image))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtf_escapes_and_keeps_paragraphs() {
        assert_eq!(
            rtf("a {b} \\c\nd\n\nGrüße"),
            "{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0 Segoe UI;}}\\f0\\fs18 \
             a \\{b\\} \\\\c d\\par\\par Gr\\u252?\\u223?e}"
        );
        // Outside the 16-bit range: two code units, the first negative as RTF wants it.
        assert!(rtf("😀").contains("\\u-10179?\\u-8704?"));
    }

    #[test]
    fn zip_holds_the_files_in_a_folder() {
        let dir = std::env::temp_dir().join(format!("xtask-zip-{}", std::process::id()));
        let stage = dir.join("stage");
        fs::create_dir_all(&stage).unwrap();
        fs::write(stage.join("xml-atlas.exe"), b"MZ program").unwrap();
        fs::write(stage.join("LICENSE-MIT"), "MIT\r\n").unwrap();
        let path = dir.join("out.zip");
        let files = ["xml-atlas.exe".to_string(), "LICENSE-MIT".to_string()];
        write_zip(&path, "xml-atlas-1.2.3", &stage, &files).unwrap();
        let mut zip = zip::ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
        let names: Vec<String> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"xml-atlas-1.2.3/xml-atlas.exe".to_string()));
        let mut text = String::new();
        std::io::Read::read_to_string(
            &mut zip.by_name("xml-atlas-1.2.3/LICENSE-MIT").unwrap(),
            &mut text,
        )
        .unwrap();
        assert_eq!(text, "MIT\r\n");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn dates_from_days() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_407), (2025, 11, 15));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }

    /// Windows Installer saves each icon under its Id, and the shell reads it by that name's
    /// extension; an Id without one leaves the Start menu entry blank.
    #[test]
    fn installer_icons_keep_their_extension() {
        let wxs = include_str!("../../packaging/windows/xml-atlas.wxs");
        let mut icons = 0;
        for (i, _) in wxs.match_indices("<Icon ") {
            let rest = &wxs[i..];
            let id = rest
                .split_once("Id=\"")
                .and_then(|(_, after)| after.split_once('"'))
                .map(|(id, _)| id)
                .expect("Icon without Id");
            assert!(
                id.ends_with(".ico") || id.ends_with(".exe"),
                "Icon Id {id:?} needs the .ico or .exe extension"
            );
            assert!(
                wxs.contains(&format!("Icon=\"{id}\"")),
                "the shortcut does not use {id:?}"
            );
            icons += 1;
        }
        assert_eq!(icons, 1);
    }

    /// WixUI_Advanced's own all-users folder comes from ProgramFilesFolder while the install
    /// is still per user, which Windows Installer redirects to %LocalAppData%\Programs.
    #[test]
    fn all_users_install_goes_to_program_files() {
        let wxs = include_str!("../../packaging/windows/xml-atlas.wxs");
        let set = |id: &str| {
            let start = wxs
                .find(&format!("<SetProperty Id=\"{id}\""))
                .unwrap_or_else(|| panic!("{id} is not set"));
            let end = start + wxs[start..].find("/>").unwrap();
            &wxs[start..end]
        };
        let machine = set("WixPerMachineFolder");
        assert!(machine.contains("Value=\"[%ProgramW6432]\\[ApplicationFolderName]\\\""));
        assert!(machine.contains("After=\"WixSetDefaultPerMachineFolder\""));
        let user = set("WixPerUserFolder");
        assert!(user.contains("Value=\"[LocalAppDataFolder]Programs\\[ApplicationFolderName]\\\""));
        assert!(user.contains("After=\"WixSetDefaultPerUserFolder\""));
        assert!(set("APPLICATIONFOLDER").contains("MSIINSTALLPERUSER=1"));
    }

    #[test]
    fn icon_is_found_in_the_executable() {
        // An .ico with two images, "AB" and "CDE", after the 6-byte header and two
        // 16-byte entries (length at +8, offset at +12).
        let mut ico = vec![0, 0, 1, 0, 2, 0];
        for (len, offset) in [(2u32, 38u32), (3, 40)] {
            ico.extend_from_slice(&[16, 16, 0, 0, 1, 0, 32, 0]);
            ico.extend_from_slice(&len.to_le_bytes());
            ico.extend_from_slice(&offset.to_le_bytes());
        }
        ico.extend_from_slice(b"ABCDE");
        assert!(has_icon(b"MZ..AB..CDE..", &ico));
        assert!(!has_icon(b"MZ..AB..CD..", &ico));
        assert!(!has_icon(b"MZ", &[0, 0, 1, 0, 0, 0]));
        assert!(!has_icon(b"MZ", b"not an icon"));
    }

    #[test]
    fn crlf_once() {
        assert_eq!(crlf("a\nb\r\nc"), "a\r\nb\r\nc");
    }
}
