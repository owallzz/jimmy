#!/usr/bin/env bash
# Pack dist/jimmy.app (from make-app.sh) into dist/jimmy-<version>-macos-universal.dmg, with an
# /Applications link to drag it onto.
#
#   packaging/macos/make-dmg.sh        # DIST=dist by default, like make-app.sh
set -euo pipefail

cd "$(dirname "$0")/../.."
V="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
DIST="${DIST:-dist}"
APP="$DIST/jimmy.app"
DMG="$DIST/jimmy-$V-macos-universal.dmg"
[ -d "$APP" ] || { echo "no $APP (run make-app.sh)" >&2; exit 1; }

root="$(mktemp -d)"
ditto "$APP" "$root/jimmy.app"
ln -s /Applications "$root/Applications"

# hdiutil now and then fails with "Resource busy" on CI machines; try again.
for try in 1 2 3; do
    hdiutil create -volname "jimmy $V" -srcfolder "$root" -fs HFS+ -format UDZO \
        -imagekey zlib-level=9 -ov "$DMG" && break
    [ "$try" = 3 ] && exit 1
    sleep 5
done
rm -rf "$root"
hdiutil verify "$DMG"
echo "done: $DMG"
