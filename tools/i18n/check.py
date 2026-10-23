#!/usr/bin/env python3
"""Language check for the OmaStudio interface.

Fails when
  * a user-visible string in app/qml is not wrapped in qsTr() (and is not on the
    allowlist of names and symbols that stay the same in every language),
  * an English source string contains Turkish (the interface must not mix languages),
  * a translation in app/i18n is missing, unfinished, out of date with the QML, drops
    or adds a %1/%n placeholder, or is left in English,
  * a translation has no native name in app/qml/I18n.qml.

Usage: tools/i18n/check.py [--quiet]
"""

import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
QML_DIR = ROOT / "app" / "qml"
TS_DIR = ROOT / "app" / "i18n"
HERE = Path(__file__).resolve().parent

STRING = re.compile(r'"((?:[^"\\\n]|\\.)*)"')
TR_CALL = re.compile(r'\b(?:qsTr|QT_TR_NOOP)\(\s*"((?:[^"\\\n]|\\.)*)"(?:\s*,\s*"((?:[^"\\\n]|\\.)*)")?')
WORD = re.compile(r"[^\W\d_]{2,}", re.UNICODE)
PLACEHOLDER = re.compile(r"%(?:n|L?[1-9](?!\d))")

# Properties and model keys that hold text shown to the user
UI_PROPERTY = re.compile(
    r"^\s*(?:text|title|label|placeholderText|ToolTip\.text|Accessible\.name|"
    r"Accessible\.description|subtitle|pasteNotice|exportStatusText)\s*[:=]"
)
UI_KEYS = {"name", "sub", "desc", "badge", "tip", "tag", "label", "coverage", "title"}
KEY_BEFORE = re.compile(r'"?(\w+)"?\s*:\s*$')
UI_CALL = re.compile(r"\b(?:showToast|notify|fillText|strokeText)\(|\breturn\s")

# Lines that never reach the interface
IGNORE_LINE = re.compile(r"^\s*(?://|import\s|\*)|console\.(?:log|warn|error)")

TURKISH_CHARS = re.compile(r"[çğıöşüÇĞİÖŞÜ]")
# Letters only Turkish (and Azerbaijani) use, for spotting Turkish in other languages
TURKISH_ONLY = re.compile(r"[ğıİşĞŞ]")
TURKISH_WORDS = {"ve", "bir", "için", "yok", "basit", "tarama", "dosya", "fotoğraf", "ayarlar",
                 "yanit", "yanıt", "veremedi", "kaydedildi", "hata", "açık", "kapalı", "dışa"}
ENGLISH_WORDS = {"the", "and", "with", "your", "failed", "saved", "settings", "loading",
                 "selected", "please", "found", "photos"}
# Words shared with English in these languages
ENGLISH_WORD_EXCEPTIONS = {
    "nl": {"selected"},
}
# Scripts each language is written in; a translation with letters but none of these
# was left in English (or another language)
SCRIPT = {
    "ar": r"[؀-ۿ]", "fa": r"[؀-ۿ]", "he": r"[֐-׿]",
    "el": r"[Ͱ-Ͽ]", "ru": r"[Ѐ-ӿ]", "uk": r"[Ѐ-ӿ]",
    "hi": r"[ऀ-ॿ]", "th": r"[฀-๿]", "ja": r"[぀-ヿ一-鿿]",
    "ko": r"[가-힯]", "zh_CN": r"[一-鿿]", "zh_TW": r"[一-鿿]",
}


ESCAPE = re.compile(r"\\(u[0-9a-fA-F]{4}|.)")


def unescape(s):
    """A QML string literal's value."""
    def one(m):
        e = m.group(1)
        if e.startswith("u") and len(e) == 5:
            return chr(int(e[1:], 16))
        return {"n": "\n", "t": "\t"}.get(e, e)
    return ESCAPE.sub(one, s)


def load_list(name):
    path = HERE / name
    items = set()
    if path.exists():
        for line in path.read_text(encoding="utf-8").splitlines():
            line = line.rstrip("\n")
            if line.strip() and not line.lstrip().startswith("#"):
                items.add(line.strip())
    return items


def scan_qml(allow):
    """Untranslated user-visible literals and every qsTr source in the QML."""
    problems = []
    sources = set()
    for qml in sorted(QML_DIR.glob("*.qml")):
        context = qml.stem
        for no, line in enumerate(qml.read_text(encoding="utf-8").splitlines(), 1):
            translated = []
            for m in TR_CALL.finditer(line):
                sources.add((context, unescape(m.group(1)), unescape(m.group(2) or "")))
                translated.append((m.start(), m.end()))
            if IGNORE_LINE.search(line):
                continue
            prop = UI_PROPERTY.search(line)
            call = UI_CALL.search(line)
            for m in STRING.finditer(line):
                text = unescape(m.group(1))
                if any(a <= m.start() < b for a, b in translated):
                    continue
                if not WORD.search(text) or text.strip() in allow or re.fullmatch(r"#[0-9a-fA-F]{3,8}", text):
                    continue
                after = line[m.end():].lstrip()
                before = line[:m.start()].rstrip()
                # Object keys and comparisons
                if after.startswith(":") or before.endswith(("===", "!==", "==", "!=", "case")):
                    continue
                key = KEY_BEFORE.search(before)
                if key and not prop:
                    if key.group(1) not in UI_KEYS:
                        continue
                elif not (prop or (call and m.start() > call.start())):
                    continue
                # Identifiers passed to functions ("pick", "none"); control-socket replies
                if not prop and (re.fullmatch(r"[a-z_-]+", text) or text.startswith(("Error", "OK"))):
                    continue
                problems.append(f"{qml.relative_to(ROOT)}:{no}: not translated: \"{text}\"")
    return problems, sources


def check_sources(sources):
    problems = []
    for context, text, _ in sorted(sources):
        words = {w.lower() for w in WORD.findall(text)}
        if TURKISH_CHARS.search(text) or words & TURKISH_WORDS:
            problems.append(f"source [{context}] is not English: \"{text}\"")
    return problems


def check_ts(path, sources, identical_ok):
    code = path.stem.removeprefix("qml_")
    problems = []
    try:
        root = ET.parse(path).getroot()
    except ET.ParseError as e:
        return [f"{path.name}: {e}"]
    if code == "en":
        # Only plural forms; the English text is the source itself
        for msg in root.iter("message"):
            if msg.get("numerus") != "yes":
                problems.append(f"{path.name}: has a non-plural entry \"{msg.findtext('source')}\"")
    seen = set()
    script = re.compile(SCRIPT[code]) if code in SCRIPT else None
    english_words = ENGLISH_WORDS - ENGLISH_WORD_EXCEPTIONS.get(code, set())
    for ctx in root.iter("context"):
        cname = ctx.findtext("name")
        for msg in ctx.iter("message"):
            src = msg.findtext("source") or ""
            comment = msg.findtext("comment") or ""
            tr = msg.find("translation")
            kind = tr.get("type") if tr is not None else "missing"
            if kind in ("vanished", "obsolete"):
                problems.append(f"{path.name}: [{cname}] stale entry \"{src}\" (run tools/i18n/update.sh)")
                continue
            seen.add((cname, src, comment))
            if code == "en" and msg.get("numerus") != "yes":
                continue
            if tr is None or kind == "unfinished":
                problems.append(f"{path.name}: [{cname}] unfinished: \"{src}\"")
                continue
            forms = [f.text or "" for f in tr.iter("numerusform")] if msg.get("numerus") == "yes" else [tr.text or ""]
            if not forms or any(not f.strip() for f in forms):
                problems.append(f"{path.name}: [{cname}] empty translation: \"{src}\"")
                continue
            want = set(PLACEHOLDER.findall(src))
            for f in forms:
                have = set(PLACEHOLDER.findall(f))
                # A plural form may spell out the number ("one photo") instead of %n
                if msg.get("numerus") == "yes":
                    have.add("%n")
                if have != want:
                    problems.append(f"{path.name}: [{cname}] placeholders {sorted(have)} != {sorted(want)}: \"{src}\" -> \"{f}\"")
                if code == "en":
                    continue
                if f == src and src not in identical_ok and f"{code}:{src}" not in identical_ok:
                    problems.append(f"{path.name}: [{cname}] left in English: \"{src}\"")
                elif script and WORD.search(f) and not script.search(f) and src not in identical_ok \
                        and f"{code}:{src}" not in identical_ok:
                    problems.append(f"{path.name}: [{cname}] not in the language's script: \"{f}\"")
                elif code != "tr" and TURKISH_ONLY.search(f) and not TURKISH_ONLY.search(src):
                    problems.append(f"{path.name}: [{cname}] Turkish letters in a {code} translation: \"{f}\"")
                elif {w.lower() for w in WORD.findall(f)} & english_words and f != src:
                    problems.append(f"{path.name}: [{cname}] English words in the translation: \"{f}\"")
    # English carries only the plural forms
    missing = sources - seen if code != "en" else set()
    for context, text, comment in sorted(missing):
        problems.append(f"{path.name}: [{context}] missing \"{text}\" (run tools/i18n/update.sh)")
    return problems


def main():
    quiet = "--quiet" in sys.argv
    allow = load_list("untranslated.txt")
    identical_ok = load_list("identical.txt")
    problems, sources = scan_qml(allow)
    problems += check_sources(sources)

    ts_files = sorted(TS_DIR.glob("qml_*.ts"))
    if not any(p.stem == "qml_en" for p in ts_files):
        problems.append("app/i18n/qml_en.ts is missing")
    i18n_qml = (QML_DIR / "I18n.qml").read_text(encoding="utf-8")
    for ts in ts_files:
        code = ts.stem.removeprefix("qml_")
        if f'"{code}":' not in i18n_qml:
            problems.append(f"app/qml/I18n.qml: no native name for {code}")
        problems += check_ts(ts, sources, identical_ok)

    for p in problems:
        print(p)
    if not quiet or problems:
        langs = ", ".join(ts.stem.removeprefix("qml_") for ts in ts_files)
        print(f"{len(sources)} strings, {len(ts_files)} languages ({langs}): "
              f"{'OK' if not problems else f'{len(problems)} problem(s)'}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
