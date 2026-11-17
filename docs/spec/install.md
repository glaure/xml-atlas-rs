# Installing

How people get XML Atlas onto their computer, keep it up to date and remove it. The program
itself is one executable for the window (`xml-atlas`) and one for the command line
(`xml-atlas-cli`); there is no runtime to install alongside it.

Version 1.0 has packages for Windows and Linux; the macOS package follows after it
(`docs/decisions.md`, D20).

## Rules

### Every platform

- **PKG-1** `[built]` Every release is built by CI from a version tag and published on the
  project's GitHub releases page, with release notes and a SHA-256 checksum for each file.
  Nobody builds a release package by hand. `.github/workflows/release.yml` runs on a tag
  `vX.Y.Z`: it refuses a tag that is not the version in `Cargo.toml`, builds the `.deb` and
  `.rpm` with `cargo xtask package` and runs `packaging/linux/install-test.sh` on them,
  builds the MSI and zip on Windows with `cargo xtask package-windows`, and puts the four
  files and a `SHA256SUMS` into a draft release. Its notes say the Windows files are not
  signed (PKG-13). A maintainer checks the draft and publishes it. Run again for the same
  tag, it replaces the draft's files; it never touches a published release. A pull request
  that changes the packaging builds the packages the same way, without a release. Not yet
  run from a real tag.
- **PKG-2** `[planned]` Every package contains both programs, the license texts (MIT and
  Apache-2.0) and the licenses of the libraries compiled in. Help > About shows the version,
  and `xml-atlas-cli --version` prints it.
- **PKG-3** `[planned]` The installed program starts as fast as a local build: a small file is
  on screen within the start-up target (OPEN-21).
- **PKG-4** `[planned]` After installing, an XML file can be opened with XML Atlas from the
  file manager: *Open with* lists it for `.xml` files (and for `application/xml` and
  `text/xml` on Linux). Installing does not make XML Atlas the default program for `.xml`;
  the user chooses that.
- **PKG-5** `[built]` Installing a newer version replaces the older one in place, and
  keeps the user's settings and recent files (WIN-3). Uninstalling removes the program,
  its shortcuts and its file associations, and leaves the settings file, so a later install
  finds it again. On the owner's Windows 11 (2026-11-16) the 0.1.1 MSI installed over
  0.1.0: one XML Atlas in the installed apps, `xml-atlas-cli --version` prints 0.1.1, the
  Start menu shortcut works, and a changed setting and the recent files were kept. Not yet
  tried: an upgrade of the `.deb` and `.rpm` (PKG-30, PKG-31).
- **PKG-6** `[planned]` 64-bit x86 on every platform; 64-bit ARM on macOS (Apple silicon)
  and on Windows and Linux where CI can build it.
- **PKG-7** `[planned]` The program can tell the user that a newer version exists: at most
  once a day it asks GitHub for the latest release and, when it is newer, says so in the
  status bar with a link to the releases page. It never downloads or installs anything by
  itself, sends nothing but that request, and can be turned off in the settings (WIN-3).
  Without a network it stays silent. In a Linux package or the Flatpak the notice says to
  update with the package manager.

### Windows

- **PKG-10** `[done]` An MSI installer for Windows 10 and 11. It offers to install for the
  current user (no administrator rights needed) or for all users. It adds a Start menu entry
  "XML Atlas", an entry in Settings > Apps for uninstalling, and *Open with* for `.xml`
  (PKG-4). A checkbox, off by default, adds `xml-atlas-cli` to the PATH.
  `cargo xtask package-windows` builds it on Windows with WiX 5.0.2 from
  `packaging/windows/xml-atlas.wxs`. The installer's dialogs (WiX's Advanced set) ask for
  the current user or all users; the PATH is a feature under Customize, off unless chosen
  (`ADDLOCAL=AddToPath` on the command line). *Open with* comes from a ProgId of its own
  under `.xml`'s OpenWithProgids, and Settings > Default apps can choose it through
  RegisteredApplications; the default program stays as it was. It installs to
  `%LocalAppData%\Programs\XML Atlas` for the current user and to `C:\Program Files\XML Atlas`
  for all users. The icon is built into `xml-atlas.exe` too, for Explorer. On the owner's
  Windows 11 (2026-11-16): installed for the current user and for all users, the Start menu
  entry, the taskbar and Explorer show the icon, *Open with* opens an XML file, the PATH
  feature puts `xml-atlas-cli` on the PATH, the window also starts with
  `XML_ATLAS_RENDERER=wgpu`, and uninstalling works. The 0.1.1 MSI installed over 0.1.0
  replaces it in place and keeps the settings and recent files (PKG-5, 2026-11-16). The MSI
  is not signed yet (PKG-13).
- **PKG-11** `[done]` The MSI has an upgrade code of its own, not the Qt XML Atlas's
  (QT-13): the Qt version has no install base to carry over, so the two are separate
  programs to Windows. Ours is `7DA64DFB-4246-4B3A-B6E3-9860CABB8C81`; it never changes.
- **PKG-12** `[built]` A zip with the two executables, for use without installing
  ("portable"). It stores settings in the usual place, not next to the program. The same
  command writes it, with the license texts, in a folder named after the version. Built on
  Windows and its five files checked; not yet run from the unpacked zip.
- **PKG-13** `[planned]` (after 1.0) The executables and the MSI are signed with the
  owner's employer's code-signing certificate, so Windows does not warn that the publisher
  is unknown.
- **PKG-14** `[planned]` (after 1.0) Signing for Windows is a manual step, because the
  certificate stays with the company and never goes into CI. Until then, releases are
  published with unsigned Windows files, and their notes say so. Once signing is in place:
  1. CI builds the release from the tag as a draft on GitHub, with unsigned Windows files
     and a note that they still need signing.
  2. A maintainer with access to the certificate runs one script on a Windows machine. It
     downloads the draft's Windows files, signs `xml-atlas.exe` and `xml-atlas-cli.exe`,
     builds the MSI and the zip again from the signed executables, signs the MSI, checks
     every signature, and uploads the files and their new checksums in place of the
     unsigned ones.
  3. The maintainer publishes the draft. A release is never published with unsigned Windows
     files.
  The script is in the repository, and its steps are written down, so that someone else at
  the company can sign a release.

### macOS

- **PKG-20** `[planned]` (after 1.0, D20) A disk image (DMG) holding `XML Atlas.app`, to drag into
  Applications. One universal app for Apple silicon and Intel, for macOS 12 and later. The
  app registers for XML files (PKG-4) and opens a file dropped on its Dock icon.
- **PKG-21** `[planned]` The app is signed with the Developer ID of the owner's employer's
  Apple developer account and notarized, so Gatekeeper opens it without a warning. As for
  Windows (PKG-14), this is a manual step, and the account's credentials never go into CI:
  1. The draft release from PKG-14 step 1 also holds an unsigned DMG, with a note that it
     still needs signing.
  2. A maintainer with access to the account runs one script on a Mac. It downloads the
     DMG, signs `xml-atlas-cli` and the app with the Developer ID and the hardened runtime,
     builds the DMG again from the signed app, signs it, sends it for notarization, staples
     the ticket to it, checks the signatures and that Gatekeeper accepts it, and uploads the
     DMG and its new checksum in place of the unsigned one.
  3. Once there is a macOS package, the draft is published only when both the Windows and
     the macOS files are signed; until then, when the Windows files are. The
     Homebrew cask (PKG-22) is updated after that, with the signed DMG's checksum.
  The script is in the repository, and its steps are written down, as for PKG-14.
- **PKG-22** `[planned]` `xml-atlas-cli` is inside the app; a Homebrew cask installs the app
  and puts the command on the PATH.

### Linux

- **PKG-30** `[done]` A `.deb` package for Ubuntu 22.04 and 24.04 LTS and Debian 12 and
  later, installable with `apt install ./xml-atlas_….deb`.
- **PKG-31** `[done]` An `.rpm` package for RHEL 9 and its rebuilds (Rocky Linux,
  AlmaLinux), and for current Fedora, installable with `dnf install ./xml-atlas-….rpm`.
- **PKG-32** `[planned]` After the `.deb` and `.rpm`: a Flatpak on Flathub, with the id
  `io.github.glaure.XmlAtlas`, for every other distribution and for users without
  administrator rights. It may read and write the files the user opens through the
  file dialog or the command line, and nothing else.
- **PKG-33** `[done]` The `.deb` and `.rpm` install both programs into `/usr/bin`, a
  desktop entry and icon (so XML Atlas appears in the application menu), and the MIME
  association of PKG-4. They depend only on libraries the distribution provides (the
  graphics, Wayland and X11 libraries the window needs), declared so the package manager
  installs them. One build runs on the oldest supported release of each family. The desktop
  entry and icon use the id `io.github.glaure.XmlAtlas`. There is no AppImage or Snap
  unless someone asks for one. `cargo xtask package` builds both into `target/dist`, against
  glibc 2.34 (`docs/decisions.md`, D22); the window sets the same id, so the desktop shows
  it with the entry's icon. `packaging/linux/install-test.sh` installs each package in a
  fresh container of Ubuntu 22.04 and 24.04, Debian 12, Rocky Linux 9 and Fedora 43: the
  package manager finds every dependency, both programs run (also on Rocky's glibc 2.34),
  the window opens and draws with OpenGL (on a nested X server), and removing the package
  removes its files. On the owner's Ubuntu 26.04 desktop (Wayland) the `.deb` installs, the
  menu entry starts it, the dock shows its icon, and *Open with* lists it for XML files
  (2026-11-15). Not yet tried: an upgrade over an older version. dnf notes that the package
  is not signed (OpenPGP).
- **PKG-34** `[planned]` The window works under both Wayland and X11, and follows the
  desktop's light or dark setting (WIN-2), in every package.

## Acceptance tests

On a fresh virtual machine of each supported system, for each package:

- Install without errors. No warning about the publisher once signing is in place (PKG-13,
  after 1.0; PKG-21). XML Atlas appears in the Start menu, Applications or the application
  menu.
- Right-click an XML file, *Open with* > XML Atlas: it opens. Double-clicking it still opens
  the previous default program.
- `xml-atlas-cli --version` in a terminal prints the release's version (Windows: after
  ticking the PATH option).
- Change the layout, quit, install the next version over it: the layout is kept.
- Uninstall: the program, menu entry and *Open with* entry are gone; the settings file is
  still there.
- Windows (after 1.0): Properties > Digital Signatures of `xml-atlas.exe`,
  `xml-atlas-cli.exe` and the MSI shows the company's certificate (PKG-13). A draft release
  with unsigned files cannot be mistaken for a published one (PKG-14).
- macOS: on a Mac that has never run XML Atlas, open the downloaded DMG and start the app:
  no Gatekeeper warning. `spctl --assess -vv` on the app names the company's Developer ID and
  says "Notarized Developer ID" (PKG-21).
- With an older version installed and a newer one released: the status bar says so once,
  with a link (PKG-7). Turned off in the settings: nothing is shown and no request is made.

## Seen in the reference programs

- Qt XML Atlas: Windows MSI only, with a fixed upgrade code and a Start menu shortcut; a
  Flatpak was prepared but never finished (QT-13). PKG-10 continues from it; PKG-11 differs
  on purpose. The other platforms are new.
- XML Marker: not observed. It is a Windows program; how it installs is not written down
  yet.

## Decided

- Packages for Windows, macOS and Linux, Linux at least for Ubuntu, Debian and RHEL. (Owner,
  2026-11-15.)
- No install base to carry over from the Qt XML Atlas: the MSI does not replace it (PKG-11).
  (Owner, 2026-11-15.)
- The owner's employer signs for Windows and Apple; Windows signing is a manual step
  (PKG-13, PKG-14, PKG-21). (Owner, 2026-11-15.)
- Windows signing is postponed until after 1.0: it cannot run on GitHub, and 1.0 ships
  unsigned Windows files (PKG-13, PKG-14). (Owner, 2026-11-17.)
- macOS signing and notarization are a manual step too, not in CI (PKG-21). (Owner,
  2026-11-15.)
- Linux: `.deb` and `.rpm` first, Flatpak next, no AppImage or Snap (PKG-30 to PKG-33).
  (Owner, 2026-11-15.)
- A notice about new versions is wanted; never an automatic download (PKG-7). (Owner,
  2026-11-15.)
- Application id `io.github.glaure.XmlAtlas` (PKG-32, PKG-33). (Owner, 2026-11-15.)

## Questions

None open.
