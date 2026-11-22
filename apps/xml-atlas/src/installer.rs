//! Handing a newer version to the system's installer (`docs/spec/install.md`, PKG-8;
//! `docs/decisions.md`, D43).
//!
//! Only a click starts it. The package of the kind installed is read into memory, compared
//! with GitHub's SHA-256 and only then written to the downloads folder and opened with the
//! system's installer, which asks for any rights it needs: the program never runs as, or
//! starts anything as, an administrator. Where that cannot work, the release's page is the
//! way.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use crate::update::{self, Notice, RELEASES, Release, Version};

/// The largest package taken; the DMG, the largest, has 22 MiB.
const PACKAGE_LIMIT: u64 = 100 * 1024 * 1024;
/// Read at a time, between two looks at Cancel.
const CHUNK: usize = 64 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);
/// The whole package, on a slow connection.
const BODY_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// `xdg-open` reports a failure at once; without a desktop's own opener it runs as long as
/// the installer does.
const OPEN_WAIT: Duration = Duration::from_secs(3);
/// Desktop entries of installers, by the start of their names. An allow list: File Roller,
/// Ark and browsers register for `.deb` and `.rpm` as well, and install nothing.
const INSTALLERS: [&str; 6] = [
    "snap-store_",
    "gnome-software-local-file",
    "org.kde.discover",
    "gdebi",
    "qapt-deb-installer",
    "captain",
];

/// The packages that can be handed over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Deb,
    Rpm,
    /// Installed for the current user. One for all users gets the link only: opened, the MSI
    /// installs for the current user and would leave the other copy where it is.
    Msi,
    Dmg,
}

/// What finding out how the program was installed looks at.
struct Place<'a> {
    os: &'a str,
    arch: &'a str,
    exe: &'a Path,
    local_app_data: Option<&'a Path>,
}

/// The file of a release for this installation.
#[derive(Clone, Debug)]
struct Package {
    kind: Kind,
    version: Version,
    name: String,
    size: u64,
    sha256: String,
    /// The release's page, for when this does not work.
    page: String,
}

/// What a click on the notice asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Start,
    Cancel,
    CloseAndInstall,
}

enum Done {
    /// The installer has it.
    Opened,
    /// It waits for the program to close.
    Ready(PathBuf),
}

struct Failure {
    why: String,
    /// The system has nothing that installs it: the page opens by itself.
    open_page: bool,
}

struct Download {
    package: Package,
    rx: Receiver<Result<Done, Failure>>,
    got: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
enum State {
    #[default]
    Idle,
    Offered(Package),
    Downloading(Download),
    Ready(Package, PathBuf),
    Opened(Package),
    Failed(Package, String),
}

/// The update on its way to the system's installer.
#[derive(Default)]
pub struct Installer {
    state: State,
    /// *Close and install* was clicked and quitting has not been given up.
    closing: Option<(Kind, PathBuf)>,
}

impl Kind {
    /// How this program was installed, when its update can be handed over.
    fn detect() -> Option<Self> {
        if std::env::var_os("FLATPAK_ID").is_some() {
            return None;
        }
        let exe = std::env::current_exe().ok()?;
        let local_app_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        let place = Place {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            exe: &exe,
            local_app_data: local_app_data.as_deref(),
        };
        Self::of(&place, Path::exists)
    }

    fn of(place: &Place<'_>, exists: impl Fn(&Path) -> bool) -> Option<Self> {
        let exe = place.exe;
        match (place.os, place.arch) {
            ("linux", "x86_64") if exe == Path::new("/usr/bin/xml-atlas") => {
                if exists(Path::new("/var/lib/dpkg/info/xml-atlas.list")) {
                    Some(Kind::Deb)
                } else if exists(Path::new("/usr/share/licenses/xml-atlas/LICENSE-MIT"))
                    && ["/usr/lib/sysimage/rpm", "/var/lib/rpm"]
                        .iter()
                        .any(|db| exists(Path::new(db)))
                {
                    Some(Kind::Rpm)
                } else {
                    None
                }
            }
            ("windows", "x86_64") => {
                let folder = exe.parent()?;
                let per_user = place.local_app_data?.join("Programs").join("XML Atlas");
                let lower = |p: &Path| p.to_string_lossy().to_lowercase();
                // The zip has no icon file beside the program.
                (lower(folder) == lower(&per_user) && exists(&folder.join("xml-atlas.ico")))
                    .then_some(Kind::Msi)
            }
            ("macos", _) => {
                let macos = exe.parent()?;
                let contents = macos.parent()?;
                let app = contents.parent()?;
                (macos.ends_with("Contents/MacOS") && app.extension().is_some_and(|e| e == "app"))
                    .then_some(Kind::Dmg)
            }
            _ => None,
        }
    }

    /// The file's name, as the release workflow writes it.
    fn file_name(self, version: Version) -> String {
        match self {
            Kind::Deb => format!("xml-atlas_{version}-1_amd64.deb"),
            Kind::Rpm => format!("xml-atlas-{version}-1.x86_64.rpm"),
            Kind::Msi => format!("xml-atlas-{version}-x64.msi"),
            Kind::Dmg => format!("xml-atlas-{version}-macos-universal.dmg"),
        }
    }

    /// The type whose handler on Linux has to be an installer.
    fn mime(self) -> Option<&'static str> {
        match self {
            Kind::Deb => Some("application/vnd.debian.binary-package"),
            Kind::Rpm => Some("application/x-rpm"),
            Kind::Msi | Kind::Dmg => None,
        }
    }

    /// Windows and macOS do not replace a program that runs.
    fn closes_first(self) -> bool {
        matches!(self, Kind::Msi | Kind::Dmg)
    }
}

impl Package {
    fn of(release: &Release, kind: Kind) -> Option<Self> {
        let name = kind.file_name(release.version);
        let file = release.files.iter().find(|f| f.name == name)?;
        (file.size > 0 && file.size <= PACKAGE_LIMIT).then(|| Package {
            kind,
            version: release.version,
            size: file.size,
            sha256: file.sha256.clone(),
            page: release.url.clone(),
            name,
        })
    }

    /// Built here rather than taken from the answer, so it can only be this project's.
    fn url(&self) -> String {
        format!("{RELEASES}/download/v{}/{}", self.version, self.name)
    }
}

impl Installer {
    /// What the last check found. A download under way, or waiting, stays.
    pub fn offer(&mut self, newer: Option<&Release>) {
        if matches!(self.state, State::Downloading(_) | State::Ready(..)) {
            return;
        }
        self.state = newer
            .and_then(|release| Package::of(release, Kind::detect()?))
            .map_or(State::Idle, State::Offered);
    }

    /// Whether it has something to say without a notice of the check.
    pub fn is_active(&self) -> bool {
        !matches!(self.state, State::Idle | State::Offered(_))
    }

    /// The status bar's update notice, right to left.
    pub fn show(&self, ui: &mut egui::Ui, notice: Option<&Notice>) -> Option<Request> {
        let mut request = None;
        match &self.state {
            State::Idle => {
                if let Some(notice) = notice {
                    notice.show(ui);
                }
            }
            State::Offered(package) => {
                let notice = notice?;
                if matches!(notice, Notice::Newer(..))
                    && ui
                        .small_button("Download and install…")
                        .on_hover_text(format!(
                            "Download {} ({:.1} MiB), check it and open it with the system's \
                             installer",
                            package.name,
                            package.size as f64 / 1_048_576.0
                        ))
                        .clicked()
                {
                    request = Some(Request::Start);
                }
                notice.show(ui);
            }
            State::Downloading(d) => {
                if ui.small_button("Cancel").clicked() {
                    request = Some(Request::Cancel);
                }
                let percent = d.got.load(Ordering::Relaxed) * 100 / d.package.size;
                ui.label(format!(
                    "Downloading XML Atlas {}: {percent}%",
                    d.package.version
                ));
            }
            State::Ready(package, path) => {
                if ui
                    .small_button("Close and install…")
                    .on_hover_text(
                        "Close XML Atlas, asking about unsaved changes, and open the installer",
                    )
                    .clicked()
                {
                    request = Some(Request::CloseAndInstall);
                }
                ui.label(format!("XML Atlas {} is downloaded", package.version))
                    .on_hover_text(path.display().to_string());
            }
            State::Opened(package) => {
                ui.label(format!(
                    "Installer opened for XML Atlas {}: restart XML Atlas afterwards",
                    package.version
                ));
            }
            State::Failed(package, why) => {
                ui.hyperlink_to("Open the download page", &package.page);
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    format!("Cannot install XML Atlas {}", package.version),
                )
                .on_hover_text(why);
            }
        }
        request
    }

    /// The click on *Download and install*.
    pub fn start(&mut self, ctx: &egui::Context) {
        let State::Offered(package) = &self.state else {
            return;
        };
        let package = package.clone();
        let (tx, rx) = channel();
        let got = Arc::new(AtomicU64::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let spawned = std::thread::Builder::new()
            .name("update-download".into())
            .spawn({
                let (package, got, cancel) = (package.clone(), got.clone(), cancel.clone());
                let repaint = ctx.clone();
                move || {
                    let progress = |bytes: u64| {
                        let before = got.swap(bytes, Ordering::Relaxed);
                        if before * 100 / package.size != bytes * 100 / package.size {
                            repaint.request_repaint();
                        }
                    };
                    // The window may be gone, or the download cancelled: then nobody waits.
                    let _ = tx.send(hand_over(&package, &cancel, progress));
                    repaint.request_repaint();
                }
            });
        self.state = match spawned {
            Ok(_) => State::Downloading(Download {
                package,
                rx,
                got,
                cancel,
            }),
            Err(e) => State::Failed(package, format!("Cannot start the download: {e}")),
        };
    }

    /// Takes the download's end when it has come. The page to open, if it ended for lack of
    /// an installer.
    pub fn poll(&mut self) -> Option<String> {
        let State::Downloading(d) = &self.state else {
            return None;
        };
        let result = match d.rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err(failed("The download stopped".into())),
        };
        let package = d.package.clone();
        let mut page = None;
        self.state = match result {
            Ok(Done::Opened) => State::Opened(package),
            Ok(Done::Ready(path)) => State::Ready(package, path),
            Err(failure) => {
                log::warn!("update: {}", failure.why);
                page = failure.open_page.then(|| package.page.clone());
                State::Failed(package, failure.why)
            }
        };
        page
    }

    /// Cancel while downloading: back to the offer.
    pub fn cancel(&mut self) {
        if let State::Downloading(d) = &self.state {
            d.cancel.store(true, Ordering::Relaxed);
            self.state = State::Offered(d.package.clone());
        }
    }

    /// The notice's ×.
    pub fn dismiss(&mut self) {
        self.cancel();
        self.state = State::Idle;
        self.closing = None;
    }

    /// The click on *Close and install*: the app asks to quit next.
    pub fn close_and_install(&mut self) {
        if let State::Ready(package, path) = &self.state {
            self.closing = Some((package.kind, path.clone()));
        }
    }

    /// Quitting was given up: a later quit installs nothing.
    pub fn quit_cancelled(&mut self) {
        self.closing = None;
    }

    /// The program ends; `quitting` when it was asked to and every question was answered.
    pub fn on_exit(&mut self, quitting: bool) {
        self.cancel();
        if let Some((kind, path)) = self.take_for_exit(quitting)
            && let Err(e) = open(kind, &path)
        {
            log::error!("update: {e}");
        }
    }

    fn take_for_exit(&mut self, quitting: bool) -> Option<(Kind, PathBuf)> {
        self.closing.take().filter(|_| quitting)
    }
}

/// All that happens on the download's thread.
fn hand_over(
    package: &Package,
    cancel: &AtomicBool,
    progress: impl FnMut(u64),
) -> Result<Done, Failure> {
    // Before downloading: without an installer the browser does that.
    if let Some(mime) = package.kind.mime() {
        let handler = output_of("xdg-mime", &["query", "default", mime]).unwrap_or_default();
        if !is_installer(&handler) {
            return Err(Failure {
                why: format!(
                    "Found no installer here for {}, so the download page was opened",
                    package.name
                ),
                open_page: true,
            });
        }
    }
    let bytes = fetch(package, cancel, progress).map_err(failed)?;
    let folder = downloads().ok_or_else(|| failed("No folder for downloads".into()))?;
    let path = keep(&folder, &package.name, &bytes)
        .map_err(|e| failed(format!("Cannot write to {}: {e}", folder.display())))?;
    mark_downloaded(package.kind, &path).map_err(failed)?;
    if cancel.load(Ordering::Relaxed) {
        return Err(failed("Cancelled".into()));
    }
    if package.kind.closes_first() {
        return Ok(Done::Ready(path));
    }
    match open(package.kind, &path) {
        Ok(()) => Ok(Done::Opened),
        Err(why) => Err(Failure {
            why,
            open_page: true,
        }),
    }
}

fn failed(why: String) -> Failure {
    Failure {
        why,
        open_page: false,
    }
}

fn is_installer(desktop_entry: &str) -> bool {
    let name = desktop_entry.trim().to_lowercase();
    INSTALLERS.iter().any(|known| name.starts_with(known))
}

/// The package in memory, checked. HTTPS only, through GitHub's redirect to its file server
/// too, with the system's certificates, as the check's request (D30).
fn fetch(
    package: &Package,
    cancel: &AtomicBool,
    progress: impl FnMut(u64),
) -> Result<Vec<u8>, String> {
    let agent = update::agent()
        .https_only(true)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_recv_response(Some(ANSWER_TIMEOUT))
        .timeout_recv_body(Some(BODY_TIMEOUT))
        .build()
        .new_agent();
    let response = agent.get(package.url()).call().map_err(|e| e.to_string())?;
    read_checked(
        response.into_body().into_reader(),
        package,
        cancel,
        progress,
    )
}

/// `from` to its end, if it has the package's size and SHA-256.
fn read_checked(
    from: impl Read,
    package: &Package,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64),
) -> Result<Vec<u8>, String> {
    // One byte more shows a file that is too long, without reading all of it.
    let mut from = from.take(package.size + 1);
    let mut bytes = Vec::new();
    let mut chunk = vec![0; CHUNK];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        match from.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                bytes.extend_from_slice(&chunk[..n]);
                progress((bytes.len() as u64).min(package.size));
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(format!("The download broke off: {e}")),
        }
    }
    if bytes.len() as u64 != package.size || sha256(&bytes) != package.sha256 {
        return Err("The download differs from what GitHub lists for it; nothing was saved".into());
    }
    Ok(bytes)
}

fn sha256(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The downloads folder: where the user looks for it, and where a sandboxed installer such
/// as App Center may be given it from.
fn downloads() -> Option<PathBuf> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
    let home = PathBuf::from(home);
    if cfg!(target_os = "linux")
        && let Some(named) = output_of("xdg-user-dir", &["DOWNLOAD"])
    {
        let named = PathBuf::from(named.trim_end_matches('\n'));
        // It names the home folder itself when none is set.
        if named.is_absolute() && named != home {
            return Some(named);
        }
    }
    Some(home.join("Downloads"))
}

/// Writes `bytes` as `name` in `folder`, or beside it as `name (2)` when another file has
/// that name: nothing of the user's is overwritten. The same file there is used again.
fn keep(folder: &Path, name: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(folder)?;
    let (stem, extension) = name.rsplit_once('.').unwrap_or((name, ""));
    for n in 1..100 {
        let path = folder.join(if n == 1 {
            name.to_owned()
        } else {
            format!("{stem} ({n}).{extension}")
        });
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                if let Err(e) = file.write_all(bytes).and_then(|()| file.sync_all()) {
                    drop(file);
                    let _ = std::fs::remove_file(&path);
                    return Err(e);
                }
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let same = std::fs::metadata(&path)
                    .is_ok_and(|m| m.is_file() && m.len() == bytes.len() as u64)
                    && std::fs::read(&path).is_ok_and(|there| there == bytes);
                if same {
                    return Ok(path);
                }
            }
            Err(e) => return Err(e),
        }
    }
    Err(std::io::ErrorKind::AlreadyExists.into())
}

/// Says that the file came from the internet, as a browser does. Without it SmartScreen and
/// Gatekeeper would not look at it, and our files are unsigned (PKG-13, PKG-21).
fn mark_downloaded(kind: Kind, path: &Path) -> Result<(), String> {
    let cannot = |e: String| format!("Cannot mark the file as downloaded: {e}");
    match kind {
        Kind::Deb | Kind::Rpm => Ok(()),
        Kind::Msi => {
            let mut stream = path.as_os_str().to_owned();
            stream.push(":Zone.Identifier");
            std::fs::write(stream, "[ZoneTransfer]\r\nZoneId=3\r\n")
                .map_err(|e| cannot(e.to_string()))
        }
        Kind::Dmg => {
            let mark = format!("0081;{:x};XML Atlas;", update::now());
            let status = Command::new("/usr/bin/xattr")
                .args(["-w", "com.apple.quarantine", &mark])
                .arg(path)
                .status()
                .map_err(|e| cannot(e.to_string()))?;
            status
                .success()
                .then_some(())
                .ok_or_else(|| cannot(status.to_string()))
        }
    }
}

/// Opens `path` as a double click does: with the installer, which asks for what it needs.
fn open(kind: Kind, path: &Path) -> Result<(), String> {
    let mut command = match kind {
        Kind::Deb | Kind::Rpm => {
            let mut command = Command::new("xdg-open");
            command.arg(path);
            command
        }
        Kind::Msi => explorer(path),
        Kind::Dmg => {
            let mut command = Command::new("/usr/bin/open");
            command.arg(path);
            command
        }
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let cannot = |e: String| format!("Cannot open the installer for {}: {e}", path.display());
    let mut child = command.spawn().map_err(|e| cannot(e.to_string()))?;
    if kind.closes_first() {
        // The program is ending, and Explorer's exit code says nothing.
        return Ok(());
    }
    let started = Instant::now();
    while started.elapsed() < OPEN_WAIT {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(cannot(status.to_string())),
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(cannot(e.to_string())),
        }
    }
    // Waited for elsewhere, so that it does not stay behind as a zombie.
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Explorer opens the file through the shell, which heeds the mark of a download; `msiexec`
/// itself would not.
#[cfg(windows)]
fn explorer(path: &Path) -> Command {
    use std::os::windows::process::CommandExt;
    let mut argument = std::ffi::OsString::from("\"");
    argument.push(path);
    argument.push("\"");
    let mut command = Command::new("explorer.exe");
    // Always quoted: Explorer reads a comma as the end of a path.
    command.raw_arg(argument);
    command
}

#[cfg(not(windows))]
fn explorer(path: &Path) -> Command {
    let mut command = Command::new("explorer.exe");
    command.arg(path);
    command
}

fn output_of(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8(out.stdout).ok())?
}

#[cfg(test)]
impl Installer {
    /// An installer that has downloaded an MSI, for the app's tests.
    pub fn ready_for_test() -> Self {
        Self {
            state: State::Ready(
                tests::package(Kind::Msi, b"msi"),
                PathBuf::from("Grüße.msi"),
            ),
            closing: None,
        }
    }

    /// An installer with the button of a newer version, for the app's tests.
    pub fn offered_for_test() -> Self {
        Self {
            state: State::Offered(tests::package(Kind::Deb, b"deb")),
            closing: None,
        }
    }

    pub fn will_install(&self) -> bool {
        self.closing.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::{Install, ReleaseFile};

    pub(super) fn package(kind: Kind, bytes: &[u8]) -> Package {
        let version = Version(1, 4, 0);
        Package {
            kind,
            version,
            name: kind.file_name(version),
            size: bytes.len() as u64,
            sha256: sha256(bytes),
            page: format!("{RELEASES}/tag/v{version}"),
        }
    }

    /// A folder of the test's own, with a name that is not ASCII and has a space.
    fn folder(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "xml-atlas-install {test} Téléchargements {}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn only_an_install_that_can_be_updated_this_way_is_offered_it() {
        let of = |os, arch, exe: &str, there: &[&str]| {
            let place = Place {
                os,
                arch,
                exe: Path::new(exe),
                local_app_data: Some(Path::new("/Users/Zoë/AppData/Local")),
            };
            Kind::of(&place, |p| there.iter().any(|t| p == Path::new(t)))
        };
        let exe = "/usr/bin/xml-atlas";
        let dpkg = "/var/lib/dpkg/info/xml-atlas.list";
        let license = "/usr/share/licenses/xml-atlas/LICENSE-MIT";
        let rpm = "/var/lib/rpm";
        assert_eq!(of("linux", "x86_64", exe, &[dpkg]), Some(Kind::Deb));
        assert_eq!(of("linux", "x86_64", exe, &[license, rpm]), Some(Kind::Rpm));
        assert_eq!(
            of("linux", "x86_64", exe, &[license]),
            None,
            "another packager's"
        );
        assert_eq!(of("linux", "x86_64", exe, &[rpm]), None, "copied there");
        assert_eq!(of("linux", "aarch64", exe, &[dpkg]), None);
        let built = "/home/zoë/target/release/xml-atlas";
        assert_eq!(of("linux", "x86_64", built, &[dpkg]), None);
        let replaced = "/usr/bin/xml-atlas (deleted)";
        assert_eq!(
            of("linux", "x86_64", replaced, &[dpkg]),
            None,
            "already updated"
        );

        let user = "/users/zoë/appdata/local/programs/xml atlas";
        let (user_exe, user_ico) = (
            format!("{user}/xml-atlas.exe"),
            format!("{user}/xml-atlas.ico"),
        );
        assert_eq!(
            of("windows", "x86_64", &user_exe, &[&user_ico]),
            Some(Kind::Msi)
        );
        assert_eq!(
            of("windows", "x86_64", &user_exe, &[]),
            None,
            "the zip unpacked there"
        );
        let all = "/Program Files/XML Atlas";
        let (all_exe, all_ico) = (
            format!("{all}/xml-atlas.exe"),
            format!("{all}/xml-atlas.ico"),
        );
        assert_eq!(
            of("windows", "x86_64", &all_exe, &[&all_ico]),
            None,
            "all users"
        );

        let app = "/Applications/XML Atlas.app/Contents/MacOS/xml-atlas";
        assert_eq!(of("macos", "aarch64", app, &[]), Some(Kind::Dmg));
        assert_eq!(of("macos", "x86_64", built, &[]), None);
    }

    /// The names are those of the 1.3.0 release.
    #[test]
    fn the_package_of_the_kind_installed_is_taken() {
        let names = [
            (Kind::Deb, "xml-atlas_1.3.0-1_amd64.deb"),
            (Kind::Rpm, "xml-atlas-1.3.0-1.x86_64.rpm"),
            (Kind::Msi, "xml-atlas-1.3.0-x64.msi"),
            (Kind::Dmg, "xml-atlas-1.3.0-macos-universal.dmg"),
        ];
        let mut release = Release {
            version: Version(1, 3, 0),
            url: format!("{RELEASES}/tag/v1.3.0"),
            files: names
                .iter()
                .map(|(_, name)| ReleaseFile {
                    name: (*name).into(),
                    size: 7,
                    sha256: "ab".repeat(32),
                })
                .collect(),
        };
        for (kind, name) in names {
            let package = Package::of(&release, kind).unwrap();
            assert_eq!(package.url(), format!("{RELEASES}/download/v1.3.0/{name}"));
        }
        release.files[0].size = PACKAGE_LIMIT + 1;
        assert!(Package::of(&release, Kind::Deb).is_none(), "too large");
        release.files.clear();
        assert!(Package::of(&release, Kind::Rpm).is_none(), "link only");
    }

    /// What `xdg-mime query default` prints where each is the handler.
    #[test]
    fn archive_managers_are_not_installers() {
        for yes in [
            "snap-store_snap-store.desktop\n",
            "snap-store_ubuntu-software-local-file.desktop",
            "gnome-software-local-file.desktop",
            "gnome-software-local-file-packagekit.desktop",
            "org.kde.discover.desktop",
            "gdebi.desktop",
            "qapt-deb-installer.desktop",
        ] {
            assert!(is_installer(yes), "{yes}");
        }
        for no in [
            "",
            "\n",
            "org.gnome.FileRoller.desktop",
            "org.kde.ark.desktop",
            "firefox_firefox.desktop",
        ] {
            assert!(!is_installer(no), "{no}");
        }
    }

    #[test]
    fn only_the_listed_bytes_are_accepted() {
        let good: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let p = package(Kind::Deb, &good);
        let go = AtomicBool::new(false);
        let read = |bytes: &[u8]| read_checked(bytes, &p, &go, |_| {});
        let mut last = 0;
        let got = read_checked(good.as_slice(), &p, &go, |n| last = n);
        assert_eq!(got.as_deref(), Ok(good.as_slice()));
        assert_eq!(last, p.size, "progress to the end");

        let mut changed = good.clone();
        changed[123_456] ^= 1;
        assert!(read(&changed).is_err(), "one bit");
        assert!(read(&good[..199_999]).is_err(), "cut short");
        assert!(
            read(&[good.as_slice(), &[0]].concat()).is_err(),
            "one byte more"
        );
        let stop = AtomicBool::new(true);
        assert!(
            read_checked(good.as_slice(), &p, &stop, |_| {}).is_err(),
            "cancelled"
        );
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_file_of_the_users_is_never_overwritten() {
        let dir = folder("keep");
        let name = "xml-atlas_1.4.0-1_amd64.deb";
        let first = keep(&dir, name, b"new").unwrap();
        assert_eq!(first, dir.join(name));
        assert_eq!(keep(&dir, name, b"new").unwrap(), first, "used again");
        std::fs::write(&first, b"theirs").unwrap();
        let second = keep(&dir, name, b"new").unwrap();
        assert_eq!(second, dir.join("xml-atlas_1.4.0-1_amd64 (2).deb"));
        assert_eq!(std::fs::read(&first).unwrap(), b"theirs");
        assert_eq!(std::fs::read(&second).unwrap(), b"new");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// SmartScreen looks only at a file with this stream.
    #[cfg(windows)]
    #[test]
    fn a_downloaded_msi_is_marked() {
        let dir = folder("mark");
        let path = keep(&dir, "Grüße, 1.msi", b"msi").unwrap();
        mark_downloaded(Kind::Msi, &path).unwrap();
        let mut stream = path.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        assert!(
            std::fs::read_to_string(stream)
                .unwrap()
                .contains("ZoneId=3")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Gatekeeper looks only at a file with this attribute.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_downloaded_dmg_is_marked() {
        let dir = folder("mark");
        let path = keep(&dir, "Grüße 1.dmg", b"dmg").unwrap();
        mark_downloaded(Kind::Dmg, &path).unwrap();
        let out = Command::new("/usr/bin/xattr")
            .args(["-p", "com.apple.quarantine"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&out.stdout).starts_with("0081;"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// What a click on `label` in a status bar `width` wide asks for.
    fn click(installer: &Installer, notice: Option<&Notice>, label: &str) -> Option<Request> {
        let ctx = egui::Context::default();
        let width = 640.0;
        let frame = |events| {
            let input = egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 60.0),
                )),
                ..Default::default()
            };
            let mut request = None;
            let mut out = ctx.run_ui(input, |ui| {
                let layout = egui::Layout::right_to_left(egui::Align::Center);
                ui.with_layout(layout, |ui| request = installer.show(ui, notice));
            });
            out.textures_delta.clear();
            (request, out.shapes)
        };
        let (_, shapes) = frame(Vec::new());
        let mut at = None;
        for shape in shapes {
            if let egui::Shape::Text(text) = &shape.shape {
                let rect = text.galley.rect.translate(text.pos.to_vec2());
                // A galley's width is not rounded to whole pixels, as its place is.
                let inside = rect.left() >= 0.0 && rect.right() <= width + 1.0;
                assert!(inside, "outside: {rect:?}");
                if text.galley.text() == label {
                    at = Some(rect.center());
                }
            }
        }
        let pos = at?;
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![egui::Event::PointerMoved(pos), button(true)]);
        frame(vec![button(false)]).0
    }

    #[test]
    fn each_state_has_its_button() {
        let p = package(Kind::Deb, b"deb");
        let release = Release {
            version: p.version,
            url: p.page.clone(),
            files: Vec::new(),
        };
        let newer = Notice::Newer(release, Install::Download);
        let with = |state| Installer {
            state,
            closing: None,
        };
        let start = "Download and install…";
        let offered = with(State::Offered(p.clone()));
        assert_eq!(click(&offered, Some(&newer), start), Some(Request::Start));
        assert_eq!(click(&offered, Some(&Notice::UpToDate), start), None);
        assert_eq!(
            click(&with(State::Idle), Some(&newer), start),
            None,
            "link only"
        );

        let (_tx, rx) = channel();
        let downloading = with(State::Downloading(Download {
            package: p.clone(),
            rx,
            got: Arc::new(AtomicU64::new(1)),
            cancel: Arc::default(),
        }));
        assert_eq!(click(&downloading, None, "Cancel"), Some(Request::Cancel));
        let ready = Installer::ready_for_test();
        assert_eq!(
            click(&ready, None, "Close and install…"),
            Some(Request::CloseAndInstall)
        );
        for state in [State::Opened(p.clone()), State::Failed(p, "offline".into())] {
            assert_eq!(click(&with(state), None, ""), None);
        }
    }

    #[test]
    fn cancel_stops_the_thread_and_offers_again() {
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let mut installer = Installer {
            state: State::Downloading(Download {
                package: package(Kind::Deb, b"deb"),
                rx,
                got: Arc::default(),
                cancel: cancel.clone(),
            }),
            closing: None,
        };
        installer.cancel();
        assert!(cancel.load(Ordering::Relaxed));
        assert!(matches!(installer.state, State::Offered(_)));
        assert!(
            tx.send(Ok(Done::Opened)).is_err(),
            "nobody takes a late answer"
        );
        assert_eq!(installer.poll(), None);
    }

    #[test]
    fn the_page_opens_only_where_no_installer_does() {
        for open_page in [true, false] {
            let (tx, rx) = channel();
            let p = package(Kind::Rpm, b"rpm");
            let mut installer = Installer {
                state: State::Downloading(Download {
                    package: p.clone(),
                    rx,
                    got: Arc::default(),
                    cancel: Arc::default(),
                }),
                closing: None,
            };
            let why = "no".to_owned();
            assert!(tx.send(Err(Failure { why, open_page })).is_ok());
            assert_eq!(installer.poll(), open_page.then_some(p.page));
            assert!(installer.is_active());
        }
    }

    #[test]
    fn installs_on_exit_only_after_close_and_install() {
        let mut installer = Installer::ready_for_test();
        assert!(installer.take_for_exit(true).is_none(), "an ordinary quit");
        installer.close_and_install();
        assert!(installer.take_for_exit(false).is_none(), "ended otherwise");
        installer.close_and_install();
        installer.quit_cancelled();
        assert!(installer.take_for_exit(true).is_none(), "given up");
        installer.close_and_install();
        assert!(installer.take_for_exit(true).is_some());
    }
}
