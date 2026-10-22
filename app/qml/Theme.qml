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

    property color highlightClip: "#f7768e"
    property color shadowClip: "#388bfd"

    // UI Radii and Spacings
    readonly property int radiusSm: 6
    readonly property int radiusMd: 10
    readonly property int radiusLg: 14

    // Typography
    readonly property string fontFamily: "JetBrainsMono Nerd Font, JetBrains Mono, monospace"
    readonly property string monoFont: "JetBrainsMono Nerd Font, JetBrains Mono, monospace"
    readonly property string iconFont: "JetBrainsMono Nerd Font, JetBrains Mono, monospace"

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
    readonly property string iconArrowUp: "\uf062"
    readonly property string iconLayers: "\uf5fd"

    // Filesystem Paths for Omarchy System Theme
    readonly property string homeDir: Host.homeDir
    readonly property string omarchyStateDir: homeDir + "/.local/state/omarchy/current"

    property string lastLoadedRaw: ""

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
        root.highlightClip = dict["red"] || dict["bright_red"] || dict["color1"] || "#f7768e";
        root.shadowClip = dict["blue"] || dict["bright_blue"] || dict["color4"] || dict["color12"] || "#388bfd";
    }

    function reloadTheme() {
        lastLoadedRaw = "";
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
