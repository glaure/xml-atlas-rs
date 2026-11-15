# Installing

How people get XML Atlas onto their computer, keep it up to date and remove it. The program
itself is one executable for the window (`xml-atlas`) and one for the command line
(`xml-atlas-cli`); there is no runtime to install alongside it.

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

### Windows

- **PKG-10** `[planned]` An MSI installer for Windows 10 and 11. It offers to install for the
  current user (no administrator rights needed) or for all users. It adds a Start menu entry
  "XML Atlas", an entry in Settings > Apps for uninstalling, and *Open with* for `.xml`
  (PKG-4). A checkbox, off by default, adds `xml-atlas-cli` to the PATH.
- **PKG-11** `[planned]` The MSI replaces an installed Qt XML Atlas (QT-13), so its users
  move to the new version by installing it. (Owner to confirm, question 1.)
- **PKG-12** `[planned]` A zip with the two executables, for use without installing
  ("portable"). It stores settings in the usual place, not next to the program.
- **PKG-13** `[planned]` The executables and the MSI are signed, so Windows does not warn
  that the publisher is unknown. (Needs a certificate, question 2.)

### macOS

- **PKG-20** `[planned]` A disk image (DMG) holding `XML Atlas.app`, to drag into
  Applications. One universal app for Apple silicon and Intel, for macOS 12 and later. The
  app registers for XML files (PKG-4) and opens a file dropped on its Dock icon.
- **PKG-21** `[planned]` The app is signed with a Developer ID and notarized, so Gatekeeper
  opens it without a warning. (Needs an Apple developer account, question 2.)
- **PKG-22** `[planned]` `xml-atlas-cli` is inside the app; a Homebrew cask installs the app
  and puts the command on the PATH.

### Linux

- **PKG-30** `[planned]` A `.deb` package for Ubuntu 22.04 and 24.04 LTS and Debian 12 and
  later, installable with `apt install ./xml-atlas_….deb`.
- **PKG-31** `[planned]` An `.rpm` package for RHEL 9 and its rebuilds (Rocky Linux,
  AlmaLinux), and for current Fedora, installable with `dnf install ./xml-atlas-….rpm`.
- **PKG-32** `[planned]` A Flatpak on Flathub, for every other distribution and for users
  without administrator rights. It may read and write the files the user opens through the
  file dialog or the command line, and nothing else.
- **PKG-33** `[planned]` The `.deb` and `.rpm` install both programs into `/usr/bin`, a
  desktop entry and icon (so XML Atlas appears in the application menu), and the MIME
  association of PKG-4. They depend only on libraries the distribution provides (the
  graphics, Wayland and X11 libraries the window needs), declared so the package manager
  installs them. One build runs on the oldest supported release of each family.
- **PKG-34** `[planned]` The window works under both Wayland and X11, and follows the
  desktop's light or dark setting (WIN-2), in every package.

## Acceptance tests

On a fresh virtual machine of each supported system, for each package:

- Install without errors or warnings about the publisher (PKG-13, PKG-21 once signing is
  set up). XML Atlas appears in the Start menu, Applications or the application menu.
- Right-click an XML file, *Open with* > XML Atlas: it opens. Double-clicking it still opens
  the previous default program.
- `xml-atlas-cli --version` in a terminal prints the release's version (Windows: after
  ticking the PATH option).
- Change the layout, quit, install the next version over it: the layout is kept.
- Uninstall: the program, menu entry and *Open with* entry are gone; the settings file is
  still there.
- Windows, with the Qt XML Atlas MSI installed: installing XML Atlas replaces it
  (PKG-11).

## Seen in the reference programs

- Qt XML Atlas: Windows MSI only, with a fixed upgrade code and a Start menu shortcut; a
  Flatpak was prepared but never finished (QT-13). PKG-10 and PKG-11 continue from it; the
  other platforms are new.
- XML Marker: not observed. It is a Windows program; how it installs is not written down
  yet.

## Decided

- Packages for Windows, macOS and Linux, Linux at least for Ubuntu, Debian and RHEL. (Owner,
  2026-11-15.)

## Questions

1. Should the new MSI replace the Qt XML Atlas (same upgrade code, PKG-11), or install next
   to it while both exist?
2. Signing costs money: a Windows code-signing certificate (about 200 to 400 USD a year, or
   Azure Trusted Signing at about 10 USD a month) and an Apple developer account (99 USD a
   year). Without them Windows shows a SmartScreen warning and macOS refuses to open the app
   until it is allowed in System Settings. Sign from the first release, or later?
3. Which Linux formats first? A suggestion: `.deb` and `.rpm` first (the systems named),
   Flatpak next, and no AppImage or Snap unless someone asks.
4. Should the program check for new versions itself (a notice, never an automatic
   download), or leave that to the package managers and the releases page?
5. The Flatpak and the desktop entry need an application id in reverse-domain form. A
   suggestion: `io.github.glaure.XmlAtlas`.
