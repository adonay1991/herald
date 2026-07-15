#!/bin/sh
# Builds Herald.app into ~/Applications (ad-hoc signature, zero dependencies).
#
# IMPORTANT — macOS 26 (Tahoe) caveat: Notification Center registration of new
# bundles has been known to freeze. If you already have an authorized presenter
# app (check `herald doctor`), install this one ALONGSIDE it, verify
# `Herald.app/Contents/MacOS/herald-notify status` reports "authorized" AND a
# test banner is visible, and only then point [sinks.macos_native].app_path at
# it. Never delete a working presenter to install this one.
set -eu
SRC="$(cd "$(dirname "$0")" && pwd)"
APP="${1:-$HOME/Applications/Herald.app}"

mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
swiftc -O "$SRC/main.swift" -o "$APP/Contents/MacOS/herald-notify"
cp "$SRC/Info.plist" "$APP/Contents/"
codesign --force -s - "$APP"
echo "installed: $APP"
echo "next: '$APP/Contents/MacOS/herald-notify' 'Herald' 'authorization probe' && '$APP/Contents/MacOS/herald-notify' status"
