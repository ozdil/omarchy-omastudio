pragma Singleton
import QtQuick
import OmaStudio.Host

// Interface language. Translations live in app/i18n/qml_<code>.ts; the picker lists
// those compiled into the binary, each under its own name.
QtObject {
    id: root

    // Native names; tools/i18n/check.py fails when a translation has no entry here
    readonly property var nativeNames: ({
        "ar": "العربية",
        "cs": "Čeština",
        "da": "Dansk",
        "de": "Deutsch",
        "el": "Ελληνικά",
        "en": "English",
        "es": "Español",
        "fa": "فارسی",
        "fi": "Suomi",
        "fr": "Français",
        "he": "עברית",
        "hi": "हिन्दी",
        "hu": "Magyar",
        "id": "Bahasa Indonesia",
        "it": "Italiano",
        "ja": "日本語",
        "ko": "한국어",
        "nb": "Norsk bokmål",
        "nl": "Nederlands",
        "pl": "Polski",
        "pt_BR": "Português (Brasil)",
        "ro": "Română",
        "ru": "Русский",
        "sv": "Svenska",
        "th": "ไทย",
        "tr": "Türkçe",
        "uk": "Українська",
        "vi": "Tiếng Việt",
        "zh_CN": "简体中文",
        "zh_TW": "繁體中文"
    })

    readonly property var codes: Host.availableLanguages.split(",").filter(function(c) { return c.length > 0; })

    // [{ code, name }] sorted by name, "System default" first
    readonly property var languages: {
        var list = root.codes.map(function(c) { return { "code": c, "name": root.nativeNames[c] || c }; });
        list.sort(function(a, b) { return a.name.localeCompare(b.name); });
        var sys = root.nativeNames[Host.systemLanguage] || Host.systemLanguage;
        return [{ "code": "", "name": qsTr("System default (%1)").arg(sys) }].concat(list);
    }

    readonly property string current: Qt.uiLanguage
    readonly property string choice: Host.languageChoice
    readonly property bool rightToLeft: Qt.locale(Qt.uiLanguage).textDirection === Qt.RightToLeft

    function apply() {
        if (Qt.uiLanguage !== Host.uiLanguage) Qt.uiLanguage = Host.uiLanguage;
    }

    // The singleton is created while the window is still being built, so the
    // translation is in place before any component reports its first message
    Component.onCompleted: root.apply()

    // "" follows the system language again
    function setLanguage(code) {
        var saved = Host.setUiLanguage(code);
        root.apply();
        return saved;
    }

    // Scene names the engine reports in English
    function sceneType(name) {
        switch (String(name || "")) {
        case "Standard": return qsTr("Standard");
        case "Portrait": return qsTr("Portrait");
        case "Landscape / Nature": return qsTr("Landscape / Nature");
        case "Night / Low-Light": return qsTr("Night / Low-Light");
        case "Street & Architecture": return qsTr("Street & Architecture");
        case "Golden Hour / Sunset": return qsTr("Golden Hour / Sunset");
        }
        return String(name || "");
    }

    function sceneDescription(name, fallback) {
        switch (String(name || "")) {
        case "Standard": return qsTr("General balanced scene");
        case "Portrait": return qsTr("Skin tones detected. Suggested: soft contrast, warm tones and gentle clarity.");
        case "Landscape / Nature": return qsTr("Greenery or sky detected. Suggested: vivid landscape colours.");
        case "Night / Low-Light": return qsTr("Low-light scene with deep shadows. Suggested: noise reduction and moody tones.");
        case "Street & Architecture": return qsTr("Urban or architectural scene. Suggested: a muted documentary look.");
        case "Golden Hour / Sunset": return qsTr("Warm golden light detected. Suggested: warm glow and soft contrast.");
        }
        return String(fallback || "");
    }
}
