#!/bin/sh
# Refreshes the translation files in app/i18n from the QML (needs Qt's lupdate;
# Arch: pacman -S qt6-tools). `tools/i18n/update.sh xx` also starts a new language.
set -eu
cd "$(dirname "$0")/../.."

LUPDATE="${LUPDATE:-}"
if [ -z "$LUPDATE" ]; then
    for c in lupdate6 lupdate-qt6 /usr/lib/qt6/bin/lupdate lupdate; do
        if command -v "$c" >/dev/null 2>&1; then LUPDATE="$c"; break; fi
    done
fi
[ -n "$LUPDATE" ] || { echo "lupdate not found (pacman -S qt6-tools), or set LUPDATE" >&2; exit 1; }

for code in "$@"; do
    [ -f "app/i18n/qml_${code}.ts" ] || : > "app/i18n/qml_${code}.ts"
done

for ts in app/i18n/qml_*.ts; do
    code="${ts##*/qml_}"
    code="${code%.ts}"
    # English is the source language; its file only carries plural forms
    plural=""
    [ "$code" = "en" ] && plural="-pluralonly"
    [ -s "$ts" ] || rm -f "$ts"
    "$LUPDATE" -silent -no-obsolete -locations none -source-language en -target-language "$code" \
        $plural app/qml/*.qml -ts "$ts"
done

python3 tools/i18n/check.py --quiet || true
