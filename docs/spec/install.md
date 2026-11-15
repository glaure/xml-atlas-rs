# Installing

How people get XML Atlas onto their computer, keep it up to date and remove it. The program
itself is one executable for the window (`xml-atlas`) and one for the command line
(`xml-atlas-cli`); there is no runtime to install alongside it.

Version 1.0 has packages for Windows and Linux; the macOS package follows after it
(`docs/decisions.md`, D20).

## Rules

### Every platform

- **PKG-1** `[planned]` Every release is built by CI from a version tag and published on the
  project's GitHub releases page, with release notes and a SHA-256 checksum for each file.
  Nobody builds a release package by hand.
- **PKG-2** `[planned]` Every package contains both programs, the license texts (MIT and
  Apache-2.0) and the licenses of the libraries compiled in. Help > About shows the version,
  and `xml-atlas-cli --version` prints it.
- **PKG-3** `[planned]` The installed program starts as fast as a local build: a small file is
  on screen within the start-up target (OPEN-21).
- **PKG-4** `[planned]` After installing, an XML file can be opened with XML Atlas from the
  file manager: *Open with* lists it for `.xml` files (and for `application/xml` and
  `text/xml` on Linux). Installing does not make XML Atlas the default program for `.xml`;
  the user chooses that.
- **PKG-5** `[planned]` Installing a newer version replaces the older one in place, and
  keeps the user's settings and recent files (WIN-3). Uninstalling removes the program,
  its shortcuts and its file associations, and leaves the settings file, so a later install
  finds it again.
- **PKG-6** `[planned]` 64-bit x86 on every platform; 64-bit ARM on macOS (Apple silicon)
  and on Windows and Linux where CI can build it.
- **PKG-7** `[planned]` The program can tell the user that a newer version exists: at most
  once a day it asks GitHub for the latest release and, when it is newer, says so in the
  status bar with a link to the releases page. It never downloads or installs anything by
  itself, sends nothing but that request, and can be turned off in the settings (WIN-3).
  Without a network it stays silent. In a Linux package or the Flatpak the notice says to
  update with the package manager.

### Windows

- **PKG-10** `[planned]` An MSI installer for Windows 10 and 11. It offers to install for the
  current user (no administrator rights needed) or for all users. It adds a Start menu entry
  "XML Atlas", an entry in Settings > Apps for uninstalling, and *Open with* for `.xml`
  (PKG-4). A checkbox, off by default, adds `xml-atlas-cli` to the PATH.
- **PKG-11** `[planned]` The MSI has an upgrade code of its own, not the Qt XML Atlas's
  (QT-13): the Qt version has no install base to carry over, so the two are separate
  programs to Windows.
- **PKG-12** `[planned]` A zip with the two executables, for use without installing
  ("portable"). It stores settings in the usual place, not next to the program.
- **PKG-13** `[planned]` The executables and the MSI are signed with the owner's employer's
  code-signing certificate, so Windows does not warn that the publisher is unknown.
- **PKG-14** `[planned]` Signing for Windows is a manual step, because the certificate stays
  with the company and never goes into CI:
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

- **PKG-30** `[built]` A `.deb` package for Ubuntu 22.04 and 24.04 LTS and Debian 12 and
  later, installable with `apt install ./xml-atlas_….deb`.
- **PKG-31** `[built]` An `.rpm` package for RHEL 9 and its rebuilds (Rocky Linux,
  AlmaLinux), and for current Fedora, installable with `dnf install ./xml-atlas-….rpm`.
- **PKG-32** `[planned]` After the `.deb` and `.rpm`: a Flatpak on Flathub, with the id
  `io.github.glaure.XmlAtlas`, for every other distribution and for users without
  administrator rights. It may read and write the files the user opens through the
  file dialog or the command line, and nothing else.
- **PKG-33** `[built]` The `.deb` and `.rpm` install both programs into `/usr/bin`, a
  desktop entry and icon (so XML Atlas appears in the application menu), and the MIME
  association of PKG-4. They depend only on libraries the distribution provides (the
  graphics, Wayland and X11 libraries the window needs), declared so the package manager
  installs them. One build runs on the oldest supported release of each family. The desktop
  entry and icon use the id `io.github.glaure.XmlAtlas`. There is no AppImage or Snap
  unless someone asks for one. `cargo xtask package` builds both into `target/dist`, against
  glibc 2.34 (`docs/decisions.md`, D22); the window sets the same id, so the desktop shows
  it with the entry's icon. Built and inspected; not yet installed on fresh systems.
- **PKG-34** `[planned]` The window works under both Wayland and X11, and follows the
  desktop's light or dark setting (WIN-2), in every package.

## Acceptance tests

On a fresh virtual machine of each supported system, for each package:

- Install without errors or warnings about the publisher (PKG-13, PKG-21). XML Atlas appears in the Start menu, Applications or the application menu.
- Right-click an XML file, *Open with* > XML Atlas: it opens. Double-clicking it still opens
  the previous default program.
- `xml-atlas-cli --version` in a terminal prints the release's version (Windows: after
  ticking the PATH option).
- Change the layout, quit, install the next version over it: the layout is kept.
- Uninstall: the program, menu entry and *Open with* entry are gone; the settings file is
  still there.
- Windows: Properties > Digital Signatures of `xml-atlas.exe`, `xml-atlas-cli.exe` and the
  MSI shows the company's certificate (PKG-13). A draft release with unsigned files cannot be
  mistaken for a published one (PKG-14).
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
- macOS signing and notarization are a manual step too, not in CI (PKG-21). (Owner,
  2026-11-15.)
- Linux: `.deb` and `.rpm` first, Flatpak next, no AppImage or Snap (PKG-30 to PKG-33).
  (Owner, 2026-11-15.)
- A notice about new versions is wanted; never an automatic download (PKG-7). (Owner,
  2026-11-15.)
- Application id `io.github.glaure.XmlAtlas` (PKG-32, PKG-33). (Owner, 2026-11-15.)

## Questions

None open.
