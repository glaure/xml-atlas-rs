#!/bin/bash
# Install test for the Linux packages (docs/spec/install.md, PKG-30, PKG-31, PKG-33).
#
# For each system, in a fresh container: installs the package from target/dist with apt or
# dnf, checks that both programs run and the desktop files are in place, and that removing
# the package removes them. With XA_TEST_DISPLAY set to an X display on this machine (such
# as a nested Xephyr, `:9`), it also opens the window there and checks that it appears.
#
# Usage: packaging/linux/install-test.sh [IMAGE...]   (after `cargo xtask package`)
# Needs Docker.
set -u
cd "$(dirname "$0")/../.."
REPO=$PWD
IMAGES=("$@")
[ ${#IMAGES[@]} -eq 0 ] && IMAGES=(ubuntu:22.04 ubuntu:24.04 debian:12
  rockylinux/rockylinux:9 fedora:43)
DEB=$(ls target/dist/*.deb 2>/dev/null | head -1)
RPM=$(ls target/dist/*.rpm 2>/dev/null | head -1)
[ -n "$DEB" ] && [ -n "$RPM" ] || { echo "no packages in target/dist: run cargo xtask package"; exit 2; }
FAILED=0

for IMAGE in "${IMAGES[@]}"; do
  NAME=xml-atlas-install-test
  docker rm -f "$NAME" >/dev/null 2>&1
  case $IMAGE in
    ubuntu*|debian*)
      INSTALL="apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq /dist/$(basename "$DEB")"
      REMOVE='apt-get remove -y -qq xml-atlas' ;;
    *)
      INSTALL="dnf install -y -q /dist/$(basename "$RPM")"
      REMOVE='dnf remove -y -q xml-atlas' ;;
  esac
  X=()
  [ -n "${XA_TEST_DISPLAY:-}" ] && X=(-v /tmp/.X11-unix:/tmp/.X11-unix -e "DISPLAY=$XA_TEST_DISPLAY")
  docker run -d --name "$NAME" -v "$REPO/target/dist:/dist:ro" \
    -v "$REPO/crates/engine/tests/corpus:/corpus:ro" "${X[@]}" "$IMAGE" sleep infinity >/dev/null
  run() { docker exec "$NAME" bash -c "$1"; }
  fail() { echo "  FAIL: $1"; FAILED=1; }
  echo "== $IMAGE: $(run '. /etc/os-release; echo $PRETTY_NAME'), $(run 'ldd --version | head -1')"
  if run "$INSTALL" >/tmp/xml-atlas-install.log 2>&1; then
    echo "  installed"
    run 'xml-atlas-cli --version' | sed 's/^/  /'
    run 'xml-atlas-cli stats /corpus/utf8_bom_crlf.xml >/dev/null 2>&1' || fail "xml-atlas-cli stats"
    for f in /usr/bin/xml-atlas /usr/share/applications/io.github.glaure.XmlAtlas.desktop \
      /usr/share/icons/hicolor/scalable/apps/io.github.glaure.XmlAtlas.svg \
      /usr/share/metainfo/io.github.glaure.XmlAtlas.metainfo.xml; do
      run "test -f $f" || fail "missing $f"
    done
    if [ -n "${XA_TEST_DISPLAY:-}" ]; then
      docker exec -d "$NAME" bash -c 'xml-atlas /corpus/one_line.xml > /tmp/app.log 2>&1'
      sleep 6
      if xwininfo -root -tree -display "$XA_TEST_DISPLAY" | grep -q 'one_line.xml — XML Atlas'; then
        echo "  window shown"
      else
        fail "no window"; run 'cat /tmp/app.log' | tail -5
      fi
      run 'grep -i "opengl failed" /tmp/app.log' && echo "  (drawn with wgpu: OpenGL failed)"
    fi
    run "$REMOVE" >/dev/null 2>&1
    run 'test ! -e /usr/bin/xml-atlas && test ! -e /usr/share/applications/io.github.glaure.XmlAtlas.desktop' \
      && echo "  removed" || fail "files left after removing"
  else
    fail "install"; tail -15 /tmp/xml-atlas-install.log
  fi
  docker rm -f "$NAME" >/dev/null
done
exit $FAILED
