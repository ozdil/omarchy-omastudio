#!/usr/bin/env python3
"""Screenshots of the interface in each language, without the engine.

Loads app/qml with stand-ins for the OmaStudio.Host types (PySide6, not part of the
build: `pip install pyside6`) and writes one PNG per language, so translations that
overflow their buttons or panels can be spotted.

Usage: tools/i18n/preview.py OUT_DIR [code ...]   (default: every language)
"""

import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
os.environ.setdefault("QT_QUICK_BACKEND", "software")
os.environ.setdefault("QT_QUICK_CONTROLS_STYLE", "Basic")

from PySide6.QtCore import Property, QObject, QTimer, Signal, Slot  # noqa: E402
from PySide6.QtGui import QGuiApplication  # noqa: E402
from PySide6.QtQml import QQmlApplicationEngine, QmlElement, QmlSingleton  # noqa: E402
from PySide6.QtQuick import QQuickWindow  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
QML_IMPORT_NAME = "OmaStudio.Host"
QML_IMPORT_MAJOR_VERSION = 1
LANGS = sorted(p.stem.removeprefix("qml_") for p in (ROOT / "app" / "i18n").glob("qml_*.ts"))
STATE = {"lang": "en"}


@QmlElement
@QmlSingleton
class Host(QObject):
    ipcCall = Signal(int, str, str)
    uiLanguageChanged = Signal()
    languageChoiceChanged = Signal()

    @Property(str, constant=True)
    def openPath(self):
        return ""

    @Property(str, constant=True)
    def homeDir(self):
        return str(Path.home())

    @Property(str, constant=True)
    def enginePath(self):
        return "/nonexistent/omastudio-engine"

    @Property(str, constant=True)
    def version(self):
        return "preview"

    @Property(str, notify=uiLanguageChanged)
    def uiLanguage(self):
        return STATE["lang"]

    @Property(str, notify=languageChoiceChanged)
    def languageChoice(self):
        return STATE["lang"]

    @Property(str, constant=True)
    def systemLanguage(self):
        return "en"

    @Property(str, constant=True)
    def availableLanguages(self):
        return ",".join(LANGS)

    @Slot(str, result=str)
    def env(self, name):
        return os.environ.get(name, "")

    @Slot(int, str)
    def ipcReply(self, _id, _text):
        pass

    @Slot(str, result=bool)
    def setUiLanguage(self, code):
        STATE["lang"] = code or "en"
        self.uiLanguageChanged.emit()
        self.languageChoiceChanged.emit()
        return True


@QmlElement
class Process(QObject):
    started = Signal()
    lineRead = Signal(str)
    finished = Signal(int, str)
    runningChanged = Signal()

    def __init__(self, parent=None):
        super().__init__(parent)
        self._command, self._running, self._stdin, self._split, self._max = [], False, False, False, 0

    def _get_command(self):
        return self._command

    def _set_command(self, v):
        self._command = v

    command = Property("QStringList", _get_command, _set_command)

    def _get_running(self):
        return self._running

    def _set_running(self, v):
        # Nothing runs; every command finishes without output
        if v and not self._running:
            self._running = True
            self.runningChanged.emit()
            # The engine daemon stays up; other commands finish at once
            if "daemon" not in self._command:
                QTimer.singleShot(0, self._finish)

    def _finish(self):
        self._running = False
        self.runningChanged.emit()
        self.finished.emit(1, "")

    running = Property(bool, _get_running, _set_running, notify=runningChanged)
    stdinEnabled = Property(bool, lambda s: s._stdin, lambda s, v: setattr(s, "_stdin", v))
    splitLines = Property(bool, lambda s: s._split, lambda s, v: setattr(s, "_split", v))
    maxOutputBytes = Property(int, lambda s: s._max, lambda s, v: setattr(s, "_max", v))

    @Slot(str, result=bool)
    def write(self, _text):
        return True


@QmlElement
class WatchedFile(QObject):
    pathChanged = Signal()
    textChanged = Signal()
    watchChanged = Signal()
    loaded = Signal()
    loadFailed = Signal()

    def __init__(self, parent=None):
        super().__init__(parent)
        self._path, self._text, self._watch = "", "", False

    def _set_path(self, v):
        self._path = v
        self.pathChanged.emit()
        QTimer.singleShot(0, self.reload)

    path = Property(str, lambda s: s._path, _set_path, notify=pathChanged)
    text = Property(str, lambda s: s._text, notify=textChanged)
    watch = Property(bool, lambda s: s._watch, lambda s, v: setattr(s, "_watch", v), notify=watchChanged)

    @Slot()
    def reload(self):
        try:
            data = Path(self._path).read_text(encoding="utf-8")[:1 << 20]
        except OSError:
            self.loadFailed.emit()
            return
        self._text = data
        self.textChanged.emit()
        self.loaded.emit()


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    out = Path(sys.argv[1]).resolve()
    out.mkdir(parents=True, exist_ok=True)
    langs = sys.argv[2:] or LANGS
    lrelease = os.environ.get("LRELEASE") or shutil.which("pyside6-lrelease") or shutil.which("lrelease")

    tmp = Path(tempfile.mkdtemp())
    qml = tmp / "qml"
    shutil.copytree(ROOT / "app" / "qml", qml)
    (qml / "i18n").mkdir()
    for ts in (ROOT / "app" / "i18n").glob("qml_*.ts"):
        subprocess.run([lrelease, "-silent", "-nounfinished", "-removeidentical", str(ts),
                        "-qm", str(qml / "i18n" / (ts.stem + ".qm"))], check=True)

    app = QGuiApplication(sys.argv)
    engine = QQmlApplicationEngine()
    engine.load(str(qml / "Main.qml"))
    if not engine.rootObjects():
        return 1
    win = engine.rootObjects()[0]
    pending = list(langs)

    def shoot():
        if not pending:
            app.quit()
            return
        code = pending.pop(0)
        STATE["lang"] = code
        engine.setUiLanguage(code)
        # Let bindings and layouts settle before grabbing
        QTimer.singleShot(400, lambda: (QQuickWindow.grabWindow(win).save(str(out / f"{code}.png")), shoot()))

    QTimer.singleShot(500, shoot)
    app.exec()
    shutil.rmtree(tmp, ignore_errors=True)
    print(f"wrote {len(langs)} screenshot(s) to {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
