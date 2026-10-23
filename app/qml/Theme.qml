pragma Singleton
import QtQuick
import OmaStudio.Host

QtObject {
    id: root

    // Active Omarchy Theme Information
    property string themeName: "default"
    property bool isDarkTheme: true

    // Dynamic Studio Color Palette (Reactive bindings update all UI elements live)
    property color bgDark: "#13141c"
    property color bgBase: "#16161e"
    property color bgSurface: "#1a1b26"
    property color bgCard: "#202330"
    property color bgCardHover: "#282c3f"
    property color border: "#2f354a"
    property color borderLight: "#414868"

    property color textMain: "#c0caf5"
    property color textMuted: "#7982a9"
    property color textDim: "#545c7e"

    property color accent: "#7aa2f7"
    property color accentHover: "#89b4fa"
    property color accentPurple: "#bb9af7"
    property color accentMagenta: "#f778ba"
    property color accentGreen: "#9ece6a"
    property color accentOrange: "#ff9e64"
    property color accentCyan: "#7dcfff"
    property color accentYellow: "#e0af68"

    property color accentRed: "#f7768e"
    property color accentBlue: "#7aa2f7"

    // Clipping warnings keep their meaning in every theme: red highlights, blue shadows
    // (a theme's "red" can be any hue, Hackerman's is green)
    property color highlightClip: "#ff3b4e"
    property color shadowClip: "#388bfd"

    // Text and icons drawn on the accent colour
    readonly property color onAccent: luminance(accent) > 0.45 ? "#0b0c10" : "#ffffff"

    // Corners follow Hyprland's decoration:rounding (Omarchy: square)
    property int rounding: 6
    function r(n) { return Math.min(n, root.rounding); }
    readonly property int radiusSm: r(6)
    readonly property int radiusMd: r(10)
    readonly property int radiusLg: r(14)

    // Typography: Omarchy's font (omarchy-font-set writes fontconfig's monospace) and
    // its shell base size; the layout was designed at a 10 px base
    property string systemFont: "JetBrainsMono Nerd Font"
    property real fontBase: 10
    readonly property real fontScale: Math.max(0.8, Math.min(2.0, fontBase / 10))
    function px(n) { return Math.round(n * root.fontScale); }
    readonly property string fontFamily: systemFont
    readonly property string monoFont: systemFont
    // Nerd Font glyphs stay available whatever the text font is
    readonly property string iconFont: "JetBrainsMono Nerd Font"

    // Themeable Monochrome Icons (Unicode Font Glyph Standard)
    readonly property string iconCamera: "\uf030"
    readonly property string iconSliders: "\uf1de"
    readonly property string iconFolder: "\uf07c"
    readonly property string iconCloud: "\uf0c2"
    readonly property string iconExport: "\uf019"
    readonly property string iconRefresh: "\uf021"
    readonly property string iconShield: "\uf132"
    readonly property string iconSearch: "\uf002"
    readonly property string iconAi: "\uf0e7"
    readonly property string iconStar: "\uf005"
    readonly property string iconImage: "\uf03e"
    readonly property string iconUndo: "\uf0e2"
    readonly property string iconRedo: "\uf01e"
    readonly property string iconCheck: "\uf00c"
    readonly property string iconTimes: "\uf00d"
    readonly property string iconSplit: "\uf24d"
    readonly property string iconRotateLeft: "\uf2ea"
    readonly property string iconRotateRight: "\uf2f9"
    readonly property string iconRotate: "\uf01e"
    readonly property string iconExpand: "\uf065"
    readonly property string iconCompress: "\uf066"
    readonly property string iconCrop: "\uf125"
    readonly property string iconCoffee: "\uf0f4"
    readonly property string iconInfo: "\uf05a"
    readonly property string iconLanguage: "\uf1ab"
    readonly property string iconArrowUp: "\uf062"
    readonly property string iconLayers: "\uf5fd"

    // Filesystem Paths for Omarchy System Theme
    readonly property string homeDir: Host.homeDir
    readonly property string omarchyStateDir: homeDir + "/.local/state/omarchy/current"

    property string lastLoadedRaw: ""

    function luminance(c) {
        var k = Qt.lighter(c, 1.0);
        return 0.2126 * k.r + 0.7152 * k.g + 0.0722 * k.b;
    }

    function loadColors(raw) {
        if (!raw || raw.trim().length === 0 || raw === lastLoadedRaw) return;
        lastLoadedRaw = raw;

        var dict = {};
        var lines = String(raw).split("\n");
        for (var i = 0; i < lines.length; i++) {
            var line = lines[i].trim();
            if (!line || line.charAt(0) === '#') continue;
            var match = line.match(/^([A-Za-z0-9_-]+)\s*=\s*["']?([^"'\r\n]+?)["']?\s*(?:#.*)?$/);
            if (match) {
                dict[match[1].toLowerCase()] = match[2].trim();
            }
        }

        var mode = dict["mode"] || "dark";
        root.isDarkTheme = (mode !== "light");

        // Foundational colors
        var base = dict["background"] || dict["bg"] || (root.isDarkTheme ? "#16161e" : "#f0f2f5");
        var fg = dict["foreground"] || dict["fg"] || (root.isDarkTheme ? "#c0caf5" : "#1a1b26");
        var acc = dict["accent"] || dict["color4"] || dict["color6"] || "#7aa2f7";
        var sel = dict["selection"] || dict["selection_background"] || "";
        var mut = dict["muted"] || dict["color8"] || "";
        // fg/acc are hex strings; convert to color so .r/.g/.b exist for Qt.rgba below
        var fgC = Qt.lighter(fg, 1.0);
        var accC = Qt.lighter(acc, 1.0);

        root.bgBase = base;

        if (root.isDarkTheme) {
            root.bgDark = dict["dark_background"] || dict["darker_background"] || dict["color0"] || Qt.darker(base, 1.3);
            root.bgSurface = dict["lighter_background"] || Qt.lighter(base, 1.15);
            root.bgCard = (sel && sel !== base) ? sel : Qt.lighter(base, 1.3);
            root.bgCardHover = Qt.lighter(root.bgCard, 1.15);
            root.border = mut ? mut : Qt.rgba(fgC.r, fgC.g, fgC.b, 0.14);
            root.borderLight = acc ? Qt.rgba(accC.r, accC.g, accC.b, 0.35) : Qt.rgba(fgC.r, fgC.g, fgC.b, 0.22);
            root.textMain = dict["bright_foreground"] || fg;
            root.textMuted = dict["light_foreground"] || mut || Qt.rgba(fgC.r, fgC.g, fgC.b, 0.65);
            root.textDim = dict["dark_foreground"] || dict["color8"] || Qt.rgba(fgC.r, fgC.g, fgC.b, 0.40);
        } else {
            root.bgDark = dict["dark_background"] || Qt.darker(base, 1.08);
            root.bgSurface = dict["lighter_background"] || Qt.lighter(base, 1.02);
            root.bgCard = (sel && sel !== base) ? sel : Qt.darker(base, 1.04);
            root.bgCardHover = Qt.darker(root.bgCard, 1.06);
            root.border = mut ? mut : Qt.rgba(fgC.r, fgC.g, fgC.b, 0.14);
            root.borderLight = acc ? Qt.rgba(accC.r, accC.g, accC.b, 0.35) : Qt.rgba(fgC.r, fgC.g, fgC.b, 0.22);
            root.textMain = dict["bright_foreground"] || fg;
            root.textMuted = dict["light_foreground"] || mut || Qt.rgba(fgC.r, fgC.g, fgC.b, 0.65);
            root.textDim = dict["dark_foreground"] || dict["color8"] || Qt.rgba(fgC.r, fgC.g, fgC.b, 0.40);
        }

        root.accent = acc;
        root.accentHover = dict["bright_cyan"] || dict["bright_blue"] || Qt.lighter(acc, 1.2);
        root.accentPurple = dict["purple"] || dict["magenta"] || dict["color5"] || dict["color13"] || "#bb9af7";
        root.accentMagenta = dict["bright_magenta"] || dict["magenta"] || dict["color5"] || "#f778ba";
        root.accentGreen = dict["bright_green"] || dict["green"] || dict["color2"] || dict["color10"] || "#9ece6a";
        root.accentOrange = dict["orange"] || dict["color9"] || dict["color1"] || "#ff9e64";
        root.accentCyan = dict["bright_cyan"] || dict["cyan"] || dict["color6"] || dict["color14"] || "#7dcfff";
        root.accentYellow = dict["bright_yellow"] || dict["yellow"] || dict["color3"] || dict["color11"] || "#e0af68";
    }

    function reloadTheme() {
        lastLoadedRaw = "";
        root.fontProc.running = true;
        root.roundingProc.running = true;
        root.shellToml.reload();
        root.themeNameFile.reload();
        root.colorsFile.reload();
        return "Theme reloaded: " + root.themeName;
    }

    // Omarchy's colors.toml, reloaded when the theme changes
    property WatchedFile colorsFile: WatchedFile {
        path: root.omarchyStateDir + "/theme/colors.toml"
        watch: true
        onLoaded: root.loadColors(text)
    }

    // fontconfig's monospace family: omarchy-font-set rewrites fonts.conf
    property Process fontProc: Process {
        command: ["fc-match", "monospace", "-f", "%{family}"]
        onFinished: function(exitCode, output) {
            var family = String(output || "").split(",")[0].trim();
            if (exitCode === 0 && family.length > 0 && family.length < 128) root.systemFont = family;
        }
    }
    property WatchedFile fontsConf: WatchedFile {
        path: root.homeDir + "/.config/fontconfig/fonts.conf"
        watch: true
        onLoaded: if (!root.fontProc.running) root.fontProc.running = true
        onLoadFailed: if (!root.fontProc.running) root.fontProc.running = true
    }

    // Base text size of the Omarchy shell ([font] base-size in the theme's shell.toml)
    property WatchedFile shellToml: WatchedFile {
        path: root.omarchyStateDir + "/theme/shell.toml"
        watch: true
        onLoaded: {
            var section = "";
            var lines = String(text).split("\n");
            for (var i = 0; i < lines.length; i++) {
                var line = lines[i].trim();
                var head = line.match(/^\[([^\]]+)\]$/);
                if (head) { section = head[1].trim(); continue; }
                var m = line.match(/^base-size\s*=\s*([0-9.]+)/);
                if (section === "font" && m) {
                    var v = parseFloat(m[1]);
                    if (v >= 8 && v <= 24) root.fontBase = v;
                }
            }
        }
    }

    // Hyprland's corner rounding
    property Process roundingProc: Process {
        command: ["hyprctl", "-j", "getoption", "decoration:rounding"]
        onFinished: function(exitCode, output) {
            try {
                var v = JSON.parse(String(output)).int;
                if (exitCode === 0 && typeof v === "number" && v >= 0 && v <= 40) root.rounding = v;
            } catch (e) {
                // Not running under Hyprland: keep the default
            }
        }
    }

    Component.onCompleted: {
        root.fontProc.running = true;
        root.roundingProc.running = true;
    }

    // theme.name: the active theme's identifier (both files are polled by path, so
    // omarchy-theme-set swapping the whole directory is picked up too)
    property WatchedFile themeNameFile: WatchedFile {
        path: root.omarchyStateDir + "/theme.name"
        watch: true
        onLoaded: {
            var n = text.trim();
            if (n.length > 0 && n !== root.themeName) {
                root.themeName = n;
                root.lastLoadedRaw = "";
                root.colorsFile.reload();
            }
        }
    }
}
