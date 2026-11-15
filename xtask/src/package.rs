//! `cargo xtask package`: the Linux `.deb` and `.rpm` (`docs/spec/install.md`, PKG-30,
//! PKG-31, PKG-33).
//!
//! Both programs are built once against glibc 2.34 with `cargo zigbuild`, so that the same
//! executables run on the oldest supported release of each family: RHEL 9 has glibc 2.34,
//! Ubuntu 22.04 has 2.35 (`docs/decisions.md`, D22). The `.deb` is written with `dpkg-deb`,
//! the `.rpm` with the `rpm` crate, so no RPM tools are needed. The packages land in
//! `target/dist`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};
use serde_json::Value;

/// The id of the desktop entry, icon and metainfo (PKG-32, PKG-33).
const APP_ID: &str = "io.github.glaure.XmlAtlas";

/// The package's name in both formats.
const NAME: &str = "xml-atlas";

const SUMMARY: &str = "Fast viewer and editor for large XML files";

const DESCRIPTION: &str = "XML Atlas opens XML files of hundreds of megabytes in well under a \
second and shows them as a tree, a grid of the selected element's children, and the source \
text. The text can be edited, and tree and grid follow. The package holds the window \
(xml-atlas) and the command line (xml-atlas-cli).";

const HOMEPAGE: &str = "https://github.com/glaure/xml-atlas-rs";

/// The `Maintainer` of the `.deb` and the `Packager` of the `.rpm`.
const MAINTAINER: &str = "Gunther Laure <11034654+glaure@users.noreply.github.com>";

/// The target triple, and the oldest glibc the executables may need.
const TARGET: &str = "x86_64-unknown-linux-gnu";
const GLIBC: (u32, u32) = (2, 34);

/// The executables, from `target/<TARGET>/release`.
const BINARIES: [&str; 2] = ["xml-atlas", "xml-atlas-cli"];

/// The libraries the window loads at run time, by Debian package. None is linked, so
/// neither package manager can find them by itself. EGL and Wayland under Wayland; GLX, X11
/// and xcb under X11; fontconfig for the system fonts. Measured with `strace` under X11,
/// and taken from the `dlopen` names of the crates for Wayland.
const DEB_DEPENDS: &[&str] = &[
    "libc6 (>= 2.34)",
    "libegl1",
    "libgl1",
    "libwayland-client0",
    "libwayland-egl1",
    "libxkbcommon0",
    "libxkbcommon-x11-0",
    "libx11-6",
    "libx11-xcb1",
    "libxcursor1",
    "libxi6",
    "libxrandr2",
    "libxcb1",
    "libfontconfig1",
];

/// The file dialog asks the desktop portal; wgpu, where OpenGL fails (D21), needs Vulkan.
const DEB_RECOMMENDS: &[&str] = &["xdg-desktop-portal", "libvulkan1"];

/// The same libraries for the `.rpm`, by what they provide, which RHEL and Fedora share.
const RPM_REQUIRES: &[&str] = &[
    "libc.so.6(GLIBC_2.34)(64bit)",
    "libEGL.so.1()(64bit)",
    "libGL.so.1()(64bit)",
    "libwayland-client.so.0()(64bit)",
    "libwayland-egl.so.1()(64bit)",
    "libxkbcommon.so.0()(64bit)",
    "libxkbcommon-x11.so.0()(64bit)",
    "libX11.so.6()(64bit)",
    "libX11-xcb.so.1()(64bit)",
    "libXcursor.so.1()(64bit)",
    "libXi.so.6()(64bit)",
    "libXrandr.so.2()(64bit)",
    "libxcb.so.1()(64bit)",
    "libfontconfig.so.1()(64bit)",
    "hicolor-icon-theme",
];

const RPM_RECOMMENDS: &[&str] = &["xdg-desktop-portal", "vulkan-loader"];

/// A file the packages install.
struct Item {
    /// Where it goes, from `/`.
    dest: String,
    content: Vec<u8>,
    mode: u16,
    kind: Kind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Program,
    Data,
    /// A license text: under `/usr/share/doc` in the `.deb`, `/usr/share/licenses` in the
    /// `.rpm`.
    License,
}

pub fn package(root: &Path, skip_build: bool) -> anyhow::Result<()> {
    let meta = metadata(root)?;
    let version = version(&meta)?;
    if !skip_build {
        build(root)?;
    }
    let bin_dir = root.join("target").join(TARGET).join("release");
    let mut items = Vec::new();
    for bin in BINARIES {
        let path = bin_dir.join(bin);
        let content = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        check_glibc(bin, &content)?;
        items.push(Item {
            dest: format!("/usr/bin/{bin}"),
            content,
            mode: 0o755,
            kind: Kind::Program,
        });
    }
    let linux = root.join("packaging/linux");
    for (file, dest) in [
        (
            format!("{APP_ID}.desktop"),
            format!("/usr/share/applications/{APP_ID}.desktop"),
        ),
        (
            format!("{APP_ID}.svg"),
            format!("/usr/share/icons/hicolor/scalable/apps/{APP_ID}.svg"),
        ),
        (
            format!("{APP_ID}.metainfo.xml"),
            format!("/usr/share/metainfo/{APP_ID}.metainfo.xml"),
        ),
    ] {
        items.push(Item {
            dest,
            content: fs::read(linux.join(&file)).with_context(|| file.clone())?,
            mode: 0o644,
            kind: Kind::Data,
        });
    }
    for file in ["LICENSE-MIT", "LICENSE-APACHE"] {
        items.push(Item {
            dest: file.to_string(),
            content: fs::read(root.join(file))?,
            mode: 0o644,
            kind: Kind::License,
        });
    }
    items.push(Item {
        dest: "THIRD-PARTY-LICENSES.txt".to_string(),
        content: third_party_licenses(&meta)?.into_bytes(),
        mode: 0o644,
        kind: Kind::License,
    });

    let dist = root.join("target/dist");
    fs::create_dir_all(&dist)?;
    let deb = deb(&items, &version, &dist)?;
    let rpm = rpm(&items, &version, &dist)?;
    println!("wrote {}\nwrote {}", deb.display(), rpm.display());
    Ok(())
}

/// `cargo metadata` for Linux on x86-64.
fn metadata(root: &Path) -> anyhow::Result<Value> {
    let out = Command::new(env!("CARGO"))
        .current_dir(root)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--filter-platform",
            TARGET,
        ])
        .output()
        .context("running cargo metadata")?;
    if !out.status.success() {
        bail!("cargo metadata: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(serde_json::from_slice(&out.stdout)?)
}

/// The version of the `xml-atlas` package, the workspace's.
fn version(meta: &Value) -> anyhow::Result<String> {
    packages(meta)
        .find(|p| p["name"] == NAME && p["source"].is_null())
        .and_then(|p| p["version"].as_str())
        .map(str::to_string)
        .context("no xml-atlas package in the workspace")
}

fn packages(meta: &Value) -> impl Iterator<Item = &Value> {
    meta["packages"].as_array().into_iter().flatten()
}

/// Builds both programs against [`GLIBC`], stripped.
fn build(root: &Path) -> anyhow::Result<()> {
    let target = format!("{TARGET}.{}.{}", GLIBC.0, GLIBC.1);
    let mut cmd = Command::new(env!("CARGO"));
    cmd.current_dir(root).args([
        "zigbuild",
        "--release",
        "--target",
        &target,
        "--config",
        "profile.release.strip=\"symbols\"",
    ]);
    for bin in BINARIES {
        cmd.args(["-p", bin]);
    }
    let status = cmd
        .status()
        .context("running cargo zigbuild; install it with `cargo install cargo-zigbuild`")?;
    if !status.success() {
        bail!("cargo zigbuild failed; it needs zig on the PATH");
    }
    Ok(())
}

/// Refuses an executable that needs a newer glibc than [`GLIBC`]: it would not start on
/// the oldest supported systems. The versions it needs are named in it as `GLIBC_2.nn`.
fn check_glibc(name: &str, exe: &[u8]) -> anyhow::Result<()> {
    let newest = glibc_versions(exe).into_iter().max();
    match newest {
        Some(v) if v > GLIBC => bail!(
            "{name} needs glibc {}.{}, newer than {}.{}; build it with `cargo xtask package`, \
             not `cargo build`",
            v.0,
            v.1,
            GLIBC.0,
            GLIBC.1
        ),
        _ => Ok(()),
    }
}

/// The `GLIBC_x.y` versions named in `exe`.
fn glibc_versions(exe: &[u8]) -> BTreeSet<(u32, u32)> {
    const TAG: &[u8] = b"GLIBC_";
    let mut found = BTreeSet::new();
    let mut at = 0;
    while let Some(i) = exe[at..].windows(TAG.len()).position(|w| w == TAG) {
        let start = at + i + TAG.len();
        let end = exe[start..]
            .iter()
            .position(|b| !(b.is_ascii_digit() || *b == b'.'))
            .map_or(exe.len(), |n| start + n);
        let text = String::from_utf8_lossy(&exe[start..end]);
        let mut parts = text.split('.').map(|p| p.parse::<u32>().ok());
        if let (Some(Some(major)), Some(Some(minor))) = (parts.next(), parts.next()) {
            found.insert((major, minor));
        }
        at = end.max(start);
    }
    found
}

/// The licenses of every library compiled into the two programs (PKG-2): what each crate
/// ships as its license file, each text once, with the crates that ship it. Build-time
/// crates (build scripts, procedural macros) are not compiled in and are left out.
fn third_party_licenses(meta: &Value) -> anyhow::Result<String> {
    let by_id: BTreeMap<&str, &Value> = packages(meta)
        .filter_map(|p| Some((p["id"].as_str()?, p)))
        .collect();
    let nodes: BTreeMap<&str, &Value> = meta["resolve"]["nodes"]
        .as_array()
        .context("no resolve graph")?
        .iter()
        .filter_map(|n| Some((n["id"].as_str()?, n)))
        .collect();
    // From the two programs along normal dependencies.
    let mut queue: VecDeque<&str> = packages(meta)
        .filter(|p| BINARIES.contains(&p["name"].as_str().unwrap_or("")) && p["source"].is_null())
        .filter_map(|p| p["id"].as_str())
        .collect();
    let mut seen = BTreeSet::new();
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id) {
            continue;
        }
        let deps = nodes
            .get(id)
            .and_then(|n| n["deps"].as_array())
            .into_iter()
            .flatten();
        for dep in deps {
            let normal = dep["dep_kinds"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|k| k["kind"].is_null());
            if normal && let Some(pkg) = dep["pkg"].as_str() {
                queue.push_back(pkg);
            }
        }
    }
    // Each license text once, with the crates that ship it.
    let mut texts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut without: Vec<String> = Vec::new();
    let mut count = 0;
    for id in seen {
        let Some(p) = by_id.get(id) else { continue };
        let ours = p["source"].is_null();
        let proc_macro = p["targets"].as_array().into_iter().flatten().any(|t| {
            t["kind"]
                .as_array()
                .is_some_and(|k| k.iter().any(|k| k == "proc-macro"))
        });
        if ours || proc_macro {
            continue;
        }
        count += 1;
        let label = format!(
            "{} {} ({})",
            p["name"].as_str().unwrap_or("?"),
            p["version"].as_str().unwrap_or("?"),
            p["license"].as_str().unwrap_or("license not stated")
        );
        let dir = Path::new(p["manifest_path"].as_str().unwrap_or("")).with_file_name("");
        let files = license_files(&dir)?;
        if files.is_empty() {
            without.push(label.clone());
        }
        for f in files {
            let text = fs::read_to_string(&f).unwrap_or_default();
            texts
                .entry(text.trim().to_string())
                .or_default()
                .push(label.clone());
        }
    }
    let mut out = format!(
        "Libraries compiled into XML Atlas\n\n\
         XML Atlas itself is licensed under MIT OR Apache-2.0 (LICENSE-MIT, LICENSE-APACHE).\n\
         It contains the following {count} libraries. Each license text below comes after the\n\
         libraries that ship it.\n"
    );
    for (text, crates) in &texts {
        out.push_str(&format!("\n{}\n", "=".repeat(78)));
        for c in crates {
            out.push_str(&format!("{c}\n"));
        }
        out.push_str(&format!("{}\n\n{text}\n", "-".repeat(78)));
    }
    if !without.is_empty() {
        out.push_str(&format!(
            "\n{}\nThese libraries ship no license file; their license is the one named. The\n\
             Apache-2.0 text is in LICENSE-APACHE, the MIT text in LICENSE-MIT.\n\n",
            "=".repeat(78)
        ));
        for c in &without {
            out.push_str(&format!("{c}\n"));
        }
    }
    Ok(out)
}

/// The license files of a crate: those at the top of its source, and in a folder below it
/// the texts that come with bundled fonts (OFL, UFL, or a `.txt` named as a font file).
fn license_files(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| dir.display().to_string())? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_uppercase();
        let license = ["LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE"]
            .iter()
            .any(|p| name.starts_with(p));
        if entry.file_type()?.is_dir() {
            files.extend(bundled_licenses(&entry.path())?);
        } else if license {
            files.push(entry.path());
        }
    }
    files.sort();
    Ok(files)
}

/// The license texts in a folder of a crate: `.txt` files named for a license, or for a
/// font file next to them.
fn bundled_licenses(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let names: Vec<String> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    let fonts: BTreeSet<&str> = names
        .iter()
        .filter_map(|n| n.strip_suffix(".ttf").or_else(|| n.strip_suffix(".otf")))
        .collect();
    Ok(names
        .iter()
        .filter(|n| {
            let Some(stem) = n.strip_suffix(".txt") else {
                return false;
            };
            let upper = stem.to_uppercase();
            upper.contains("LICEN") || upper == "OFL" || upper == "UFL" || fonts.contains(stem)
        })
        .map(|n| dir.join(n))
        .collect())
}

/// Where a file goes in the `.deb`.
fn deb_dest(item: &Item) -> String {
    match item.kind {
        Kind::License => format!("/usr/share/doc/{NAME}/{}", item.dest),
        _ => item.dest.clone(),
    }
}

/// Writes the `.deb` with `dpkg-deb`.
fn deb(items: &[Item], version: &str, dist: &Path) -> anyhow::Result<PathBuf> {
    let stage = dist.join("deb-root");
    if stage.exists() {
        fs::remove_dir_all(&stage)?;
    }
    let mut size = 0;
    for item in items {
        let path = stage.join(deb_dest(item).trim_start_matches('/'));
        fs::create_dir_all(path.parent().expect("in a folder"))?;
        fs::write(&path, &item.content)?;
        set_mode(&path, item.mode)?;
        size += item.content.len();
    }
    // Debian wants the license in a file named `copyright`.
    let copyright = format!(
        "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\
         Upstream-Name: XML Atlas\n\
         Source: {HOMEPAGE}\n\n\
         Files: *\n\
         Copyright: The XML Atlas authors\n\
         License: MIT or Apache-2.0\n \
         The full texts are in LICENSE-MIT and LICENSE-APACHE in this folder; the libraries\n \
         compiled in are listed with their licenses in THIRD-PARTY-LICENSES.txt.\n"
    );
    let doc = stage.join(format!("usr/share/doc/{NAME}"));
    fs::write(doc.join("copyright"), copyright)?;
    set_mode(&doc.join("copyright"), 0o644)?;
    let control_dir = stage.join("DEBIAN");
    fs::create_dir_all(&control_dir)?;
    // Folders as packages have them, whatever the umask.
    dir_modes(&stage)?;
    let control = format!(
        "Package: {NAME}\n\
         Version: {version}-1\n\
         Architecture: amd64\n\
         Maintainer: {MAINTAINER}\n\
         Installed-Size: {}\n\
         Depends: {}\n\
         Recommends: {}\n\
         Section: editors\n\
         Priority: optional\n\
         Homepage: {HOMEPAGE}\n\
         Description: {SUMMARY}\n{}",
        size.div_ceil(1024),
        DEB_DEPENDS.join(", "),
        DEB_RECOMMENDS.join(", "),
        wrap(DESCRIPTION, 78),
    );
    fs::write(control_dir.join("control"), control)?;
    let out = dist.join(format!("{NAME}_{version}-1_amd64.deb"));
    let status = Command::new("dpkg-deb")
        .args(["--root-owner-group", "-Zxz", "--build"])
        .arg(&stage)
        .arg(&out)
        .status()
        .context("running dpkg-deb")?;
    if !status.success() {
        bail!("dpkg-deb failed");
    }
    fs::remove_dir_all(&stage)?;
    Ok(out)
}

/// Writes the `.rpm` with the `rpm` crate.
fn rpm(items: &[Item], version: &str, dist: &Path) -> anyhow::Result<PathBuf> {
    let mut b = rpm::PackageBuilder::new(NAME, version, "MIT OR Apache-2.0", "x86_64", SUMMARY);
    b.using_config(rpm::BuildConfig::v4().compression(rpm::CompressionType::Gzip))
        .release("1")
        .url(HOMEPAGE)
        .description(DESCRIPTION)
        .packager(MAINTAINER)
        .vendor("XML Atlas");
    let licenses = format!("/usr/share/licenses/{NAME}");
    b.with_dir_entry(rpm::FileOptions::dir(licenses.clone()).permissions(0o755))?;
    for item in items {
        let options = match item.kind {
            Kind::License => rpm::FileOptions::new(format!("{licenses}/{}", item.dest)).license(),
            _ => rpm::FileOptions::new(item.dest.clone()),
        };
        b.with_file_contents(item.content.clone(), options.permissions(item.mode))?;
    }
    for r in RPM_REQUIRES {
        b.requires(rpm::Dependency::any(*r));
    }
    for r in RPM_RECOMMENDS {
        b.recommends(rpm::Dependency::any(*r));
    }
    let out = dist.join(format!("{NAME}-{version}-1.x86_64.rpm"));
    b.build()?.write_file(&out)?;
    Ok(out)
}

/// `text` in lines of at most `width` columns, each starting with a blank, as the
/// description in a Debian control file is written.
fn wrap(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            out.push_str(&format!(" {line}\n"));
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    out.push_str(&format!(" {line}\n"));
    out
}

/// Sets every folder under `dir`, and `dir` itself, to `rwxr-xr-x`.
fn dir_modes(dir: &Path) -> anyhow::Result<()> {
    set_mode(dir, 0o755)?;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            dir_modes(&entry.path())?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u16) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode.into()))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_: &Path, _: u16) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_glibc_versions_an_executable_needs() {
        let exe = b"\0GLIBC_2.2.5\0GLIBC_2.34\0libc.so.6\0GLIBC_PRIVATE\0GLIBC_2.17";
        let found: Vec<_> = glibc_versions(exe).into_iter().collect();
        assert_eq!(found, [(2, 2), (2, 17), (2, 34)]);
        assert!(check_glibc("x", exe).is_ok());
        assert!(check_glibc("x", b"GLIBC_2.35").is_err());
        assert!(check_glibc("x", b"no libc at all").is_ok());
    }

    /// The `.rpm` holds the files where they belong, the license texts marked as such, and
    /// asks for the libraries the window loads.
    #[test]
    fn rpm_reads_back() {
        let dir = std::env::temp_dir().join(format!("xtask-rpm-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let item = |dest: &str, mode, kind| Item {
            dest: dest.to_string(),
            content: b"Gr\xC3\xBC\xC3\x9Fe\r\n".to_vec(),
            mode,
            kind,
        };
        let items = [
            item("/usr/bin/xml-atlas", 0o755, Kind::Program),
            item("/usr/share/applications/x.desktop", 0o644, Kind::Data),
            item("LICENSE-MIT", 0o644, Kind::License),
        ];
        let path = rpm(&items, "1.2.3", &dir).unwrap();
        let pkg = ::rpm::Package::open(&path).unwrap();
        let m = &pkg.metadata;
        assert_eq!(m.get_name().unwrap(), "xml-atlas");
        assert_eq!(m.get_version().unwrap(), "1.2.3");
        let files: Vec<(PathBuf, u16)> = m
            .get_file_entries()
            .unwrap()
            .iter()
            .map(|f| (f.path(), f.permissions()))
            .collect();
        assert!(files.contains(&("/usr/bin/xml-atlas".into(), 0o755)));
        assert!(files.contains(&("/usr/share/licenses/xml-atlas/LICENSE-MIT".into(), 0o644)));
        let license = m
            .get_file_entries()
            .unwrap()
            .into_iter()
            .find(|f| f.basename() == "LICENSE-MIT");
        assert!(license.unwrap().flags().contains(::rpm::FileFlags::LICENSE));
        let requires: Vec<String> = m
            .get_requires()
            .unwrap()
            .into_iter()
            .map(|d| d.name)
            .collect();
        for r in RPM_REQUIRES {
            assert!(requires.iter().any(|q| q == r), "{r}");
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn wraps_the_debian_description() {
        assert_eq!(wrap("one two three", 7), " one two\n three\n");
        assert!(
            wrap(DESCRIPTION, 78)
                .lines()
                .all(|l| l.len() <= 79 && l.starts_with(' '))
        );
    }
}
