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
    // Omarchy's urgent role: the theme's red (colour 1); destructive actions use it
    property color urgent: "#f7768e"
    property color accentBlue: "#7aa2f7"

    // Clipping warnings keep their meaning in every theme: red highlights, blue shadows
    // (a theme's "red" can be any hue, Hackerman's is green)
    property color highlightClip: "#ff3b4e"
    property color shadowClip: "#388bfd"

    // Text and icons drawn on the accent colour: whichever of white and near-black
    // contrasts more (WCAG), so a mid blue such as Catppuccin Latte's gets white text
    readonly property color onAccent: contrast(accent, "#ffffff") >= contrast(accent, "#0b0c10") ? "#ffffff" : "#0b0c10"

    // Corners follow Hyprland's decoration:rounding (Omarchy: square)
    property int rounding: 6
    function r(n) { return Math.min(n, root.rounding); }
    readonly property int radiusSm: r(6)
    readonly property int radiusMd: r(10)
    readonly property int radiusLg: r(14)

    // Typography: Omarchy's font (omarchy-font-set writes fontconfig's monospace) and
    // the shell's [font] base-size, from the theme's shell.toml with the user's
    // ~/.config/omarchy/shell.toml on top; the layout was designed at a 10 px base
    property string systemFont: "JetBrainsMono Nerd Font"
    readonly property real fontBase: {
        var v = parseFloat(shellValue("font", "base-size", "12"));
        return isFinite(v) && v >= 6 && v <= 32 ? v : 12;
    }
    readonly property real fontScale: Math.max(0.8, Math.min(2.0, fontBase / 10))
    // Nothing is drawn below 10 px at the base size: smaller text is not legible
    function px(n) { return Math.round(Math.max(n, 10) * root.fontScale); }
    readonly property string fontFamily: systemFont
    readonly property string monoFont: systemFont
    // Nerd Font glyphs stay available whatever the text font is
    readonly property string iconFont: "JetBrainsMono Nerd Font"

    // Spacing and control sizes follow the shell's [spacing] scale and, as in the
    // Omarchy shell, grow with the font unless the theme says otherwise
    readonly property real spacingScale: {
        var v = parseFloat(shellValue("spacing", "scale", "1"));
        var withFont = String(shellValue("spacing", "scale-with-font", "true")) !== "false";
        var base = isFinite(v) && v > 0 ? Math.min(v, 3) : 1;
        return withFont ? base * root.fontScale : base;
    }
    function sp(n) { return Math.round(n * root.spacingScale); }

    // Motion: off with Hyprland's animations (omarchy toggle animations, Appearance >
    // Animations); every transition then takes 0 ms and spinners stop
    property bool reduceMotion: false
    function duration(ms) { return root.reduceMotion ? 0 : ms; }

    // Omarchy shell tokens: the theme's shell.toml, overridden key by key by the
    // user's ~/.config/omarchy/shell.toml (which survives theme switches)
    property var shellTheme: ({})
    property var shellUser: ({})
    function shellValue(section, key, fallback) {
        var k = section + "." + key;
        if (root.shellUser[k] !== undefined) return root.shellUser[k];
        if (root.shellTheme[k] !== undefined) return root.shellTheme[k];
        return fallback;
    }
    function shellNumber(section, key, fallback) {
        var v = parseFloat(shellValue(section, key, ""));
        return isFinite(v) ? v : fallback;
    }
    // "section.key" -> value; strings lose their quotes, comments are dropped
    function parseShellToml(raw) {
        var out = {};
        var section = "";
        var lines = String(raw || "").split("\n");
        for (var i = 0; i < lines.length && i < 4000; i++) {
            var line = lines[i].trim();
            if (!line || line.charAt(0) === "#") continue;
            var head = line.match(/^\[([^\]]+)\]$/);
            if (head) { section = head[1].trim(); continue; }
            var m = line.match(/^([A-Za-z0-9_-]+)\s*=\s*(.+)$/);
            if (!m || section === "") continue;
            var v = m[2].trim();
            var q = v.match(/^"([^"]*)"|^'([^']*)'/);
            if (q) v = q[1] !== undefined ? q[1] : q[2];
            else v = v.replace(/\s+#.*$/, "").trim();
            out[section + "." + m[1]] = v;
        }
        return out;
    }
    // A colour token: "#rrggbb[aa]", Hyprland's "rgba(rrggbbaa)"/"rgb(rrggbb)", the
    // first stop of a gradient, or a reference such as "hyprland.active-border"
    function shellColor(v, fallback, depth) {
        var s = String(v === undefined || v === null ? "" : v).trim();
        if (s === "") return fallback;
        var first = s.split(/\s+/)[0];
        var hex = first.match(/^#([0-9A-Fa-f]{6}|[0-9A-Fa-f]{8})$/);
        if (hex) return first;
        var hypr = first.match(/^rgba?\(([0-9A-Fa-f]{6})([0-9A-Fa-f]{2})?\)$/);
        if (hypr) return "#" + (hypr[2] || "") + hypr[1];
        var ref = first.match(/^([A-Za-z0-9_-]+)\.([A-Za-z0-9_-]+)$/);
        if (ref && (depth || 0) < 3) return shellColor(shellValue(ref[1], ref[2], ""), fallback, (depth || 0) + 1);
        return fallback;
    }
    function withAlpha(c, a) {
        var k = Qt.lighter(c, 1.0);
        return Qt.rgba(k.r, k.g, k.b, Math.max(0, Math.min(1, a)));
    }

    // Interactive controls, styled like the Omarchy shell's own ([controls]): a state
    // is "normal", "hover-cursor", "focus", "selected" or "pressed"; colours default to
    // the foreground, at the shell's default strengths
    readonly property var controlDefaults: ({
        "normal": { fill: 0.04, border: 0.4, width: 1 },
        "hover-cursor": { fill: 0.08, border: 0.25, width: 1 },
        "focus": { fill: 0.08, border: 0.25, width: 1 },
        "selected": { fill: 0.18, border: 1.0, width: 0 },
        "pressed": { fill: 0.22, border: 0.25, width: 1 }
    })
    function controlColor(state) {
        return shellColor(shellValue("controls", state + "-color", ""), root.textMain);
    }
    function controlFill(state) {
        var d = root.controlDefaults[state] || root.controlDefaults["normal"];
        var a = state === "pressed" ? shellNumber("controls", "pressed-fill-alpha", d.fill)
                                    : shellNumber("controls", state + "-fill-alpha", d.fill);
        return withAlpha(controlColor(state === "pressed" ? "selected" : state), a);
    }
    function controlBorder(state) {
        var d = root.controlDefaults[state] || root.controlDefaults["normal"];
        var s = state === "pressed" ? "hover-cursor" : state;
        var c = shellColor(shellValue("controls", s + "-border", ""), controlColor(s));
        return withAlpha(c, shellNumber("controls", s + "-border-alpha", d.border));
    }
    function controlBorderWidth(state) {
        var d = root.controlDefaults[state] || root.controlDefaults["normal"];
        var s = state === "pressed" ? "hover-cursor" : state;
        return Math.max(0, Math.min(4, Math.round(shellNumber("controls", s + "-border-width", d.width))));
    }
    // Selected text in Omarchy's menus and launcher (the accent by default)
    readonly property color selectedText: shellColor(shellValue("menu", "selected-text", ""), root.accent)
    readonly property real selectionAlpha: shellNumber("controls", "selection-fill-alpha", 0.35)

    // Surfaces the shell themes separately: popups (menus, dropdowns), tooltips and
    // dialogs (polkit)
    readonly property color popupBg: withAlpha(shellColor(shellValue("popups", "background", ""), root.bgBase),
                                               shellNumber("popups", "background-alpha", 1))
    readonly property color popupText: shellColor(shellValue("popups", "text", ""), root.textMain)
    readonly property color popupBorder: withAlpha(shellColor(shellValue("popups", "border", ""), root.borderLight),
                                                   shellNumber("popups", "border-alpha", 1))
    readonly property color tooltipBg: withAlpha(shellColor(shellValue("tooltip", "background", ""), root.bgBase),
                                                 shellNumber("tooltip", "background-alpha", 0.97))
    readonly property color tooltipText: shellColor(shellValue("tooltip", "text", ""), root.textMain)
    readonly property color tooltipBorder: withAlpha(shellColor(shellValue("tooltip", "border", ""), root.border),
                                                     shellNumber("tooltip", "border-alpha", 1))
    readonly property color dialogBorder: withAlpha(shellColor(shellValue("polkit", "border", ""), root.popupBorder),
                                                    shellNumber("polkit", "border-alpha", 1))
    // Menus and pickers, as Omarchy's menu draws them: a faint row highlight and the
    // accent for the chosen row's text
    readonly property color menuBg: withAlpha(shellColor(shellValue("menu", "background", ""), root.popupBg),
                                              shellNumber("menu", "background-alpha", 1))
    readonly property color menuText: shellColor(shellValue("menu", "text", ""), root.textMain)
    readonly property color menuBorder: withAlpha(shellColor(shellValue("menu", "border", ""), root.popupBorder),
                                                  shellNumber("menu", "border-alpha", 1))
    readonly property color menuSelectedBg: withAlpha(shellColor(shellValue("menu", "selected-background", ""), root.textMain),
                                                      shellNumber("menu", "selected-background-alpha", 0.08))
    readonly property color scrim: withAlpha(shellColor(shellValue("menu", "scrim", ""), root.bgDark),
                                             shellNumber("menu", "scrim-alpha", 0.5))
    // Hyprland's active border colour: the frame of the focused window in this theme
    readonly property color activeBorder: shellColor(shellValue("hyprland", "active-border", ""), root.accent)
    readonly property int borderWidth: 1

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
    readonly property string iconStarEmpty: "\uf006"
    readonly property string iconFlag: "\uf024"
    readonly property string iconBan: "\uf05e"
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

    // WCAG 2 relative luminance and contrast ratio
    function relLum(c) {
        var k = Qt.lighter(c, 1.0);
        function ch(v) { return v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4); }
        return 0.2126 * ch(k.r) + 0.7152 * ch(k.g) + 0.0722 * ch(k.b);
    }
    function contrast(a, b) {
        var la = relLum(a), lb = relLum(b);
        return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
    }
    // `fg` moved towards `toward` (the main text colour) until it reaches `ratio` on `bg`;
    // an alpha colour is flattened onto `bg` first
    function readable(fg, bg, toward, ratio) {
        var f = Qt.lighter(fg, 1.0), g = Qt.lighter(bg, 1.0), w = Qt.lighter(toward, 1.0);
        var c = Qt.rgba(f.r * f.a + g.r * (1 - f.a), f.g * f.a + g.g * (1 - f.a), f.b * f.a + g.b * (1 - f.a), 1);
        for (var i = 0; i < 20 && contrast(c, g) < ratio; i++) {
            c = Qt.rgba(c.r + (w.r - c.r) * 0.15, c.g + (w.g - c.g) * 0.15, c.b + (w.b - c.b) * 0.15, 1);
        }
        return c;
    }
    // Secondary text stays legible in every theme: muted text meets WCAG AA (4.5:1) and
    // dim text (hints, placeholders, inactive icons) 3:1, on the panel cards where they sit
    function ensureContrast() {
        root.textMuted = readable(root.textMuted, root.bgCard, root.textMain, 4.5);
        root.textDim = readable(root.textDim, root.bgCard, root.textMain, 3.0);
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
        root.urgent = dict["red"] || dict["color1"] || "#f7768e";
        root.accentYellow = dict["bright_yellow"] || dict["yellow"] || dict["color3"] || dict["color11"] || "#e0af68";
        root.ensureContrast();
    }

    function reloadTheme() {
        lastLoadedRaw = "";
        root.fontProc.running = true;
        root.refreshHyprland();
        root.shellToml.reload();
        root.userShellToml.reload();
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

    // The Omarchy shell's tokens: the theme's shell.toml and the user's own
    property WatchedFile shellToml: WatchedFile {
        path: root.omarchyStateDir + "/theme/shell.toml"
        watch: true
        onLoaded: root.shellTheme = root.parseShellToml(text)
        onLoadFailed: root.shellTheme = ({})
    }
    property WatchedFile userShellToml: WatchedFile {
        path: root.homeDir + "/.config/omarchy/shell.toml"
        watch: true
        onLoaded: root.shellUser = root.parseShellToml(text)
        onLoadFailed: root.shellUser = ({})
    }

    // Hyprland's corner rounding and animations, read again whenever Hyprland applies
    // a new configuration (Appearance > Window Corners / Animations, theme switches)
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
    property Process animationsProc: Process {
        command: ["hyprctl", "-j", "getoption", "animations:enabled"]
        onFinished: function(exitCode, output) {
            try {
                var o = JSON.parse(String(output));
                if (exitCode === 0) root.reduceMotion = o.bool === false || o.int === 0;
            } catch (e) {
                // Not running under Hyprland: animations stay on
            }
        }
    }
    function refreshHyprland() {
        if (!root.roundingProc.running) root.roundingProc.running = true;
        if (!root.animationsProc.running) root.animationsProc.running = true;
    }
    property HyprlandEvents hyprland: HyprlandEvents {
        onConfigReloaded: root.refreshHyprland()
    }

    Component.onCompleted: {
        root.ensureContrast();
        root.fontProc.running = true;
        root.refreshHyprland();
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
