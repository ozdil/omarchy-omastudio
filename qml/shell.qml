import QtQuick
import Quickshell
import Quickshell.Io
import "theme"

ShellRoot {
    id: shellRoot

    FloatingWindow {
        id: win
        title: "OmaStudio - Lightroom-Grade Photo RAW Editor"
        implicitWidth: 1440
        implicitHeight: 920
        color: Theme.bgBase

        // 100% Solid Opaque Studio Foundation (Impervious to compositor alpha blending)
        Rectangle {
            anchors.fill: parent
            color: Theme.bgBase
            z: -1000
        }

        MainWindow {
            id: mainWin
            anchors.fill: parent
        }
    }


    // Omarchy Agent & CLI IPC Interface (Strict Type Safety & Sandboxed Execution)
    IpcHandler {
        target: "ozdil.omastudio"

        function toggle(): bool {
            win.visible = !win.visible;
            return win.visible;
        }

        function openPhoto(rawPath: string): string {
            if (!rawPath || rawPath.length === 0) return "Error: empty photo path";
            mainWin.loadPhoto(rawPath);
            return "OK";
        }

        function applyRecipe(recipeJson: string): string {
            if (!recipeJson || recipeJson.length === 0) return "Error: empty recipe json";
            try {
                var r = JSON.parse(recipeJson);
                mainWin.applyRecipeObject(r);
                return "OK";
            } catch(e) {
                return "Error: " + e;
            }
        }

        function optimizeSocial(platformCode: string): string {
            if (!platformCode || platformCode.length === 0) return "Error: empty platform code";
            mainWin.triggerSocial(platformCode);
            return "OK";
        }

        function applyPreset(presetName: string): string {
            if (!presetName || presetName.length === 0) return "Error: empty preset name";
            mainWin.applyPresetNamed(presetName);
            return "OK";
        }

        function setExposure(ev: real): string {
            mainWin.setExposureEv(ev);
            return "OK";
        }

        function setWarmth(kelvin: real): string {
            mainWin.setWarmthKelvin(kelvin);
            return "OK";
        }

        function reset(): string {
            mainWin.resetRecipe();
            return "OK";
        }

        function toggleCrop(): bool {
            return mainWin.toggleCropMode();
        }

        function toggleSplit(): bool {
            return mainWin.toggleSplitView();
        }

        function applyLut(lutName: string, intensity: real): string {
            if (!lutName || lutName.length === 0) return "Error: empty lut name";
            mainWin.activeLutName = lutName;
            mainWin.activeLutIntensity = (intensity !== undefined && intensity >= 0) ? intensity : 1.0;
            mainWin.activeLutPath = "";
            mainWin.requestRender();
            return "OK";
        }

        function switchGradeVersion(version: string): string {
            if (!version || version.length === 0) return "Error: empty version";
            mainWin.switchGradeVersion(version);
            return "OK";
        }

        function reloadTheme(): string {
            return Theme.reloadTheme();
        }

        function getTheme(): string {
            var t = {
                "name": Theme.themeName,
                "isDark": Theme.isDarkTheme,
                "bgBase": Theme.bgBase.toString(),
                "bgDark": Theme.bgDark.toString(),
                "bgSurface": Theme.bgSurface.toString(),
                "accent": Theme.accent.toString(),
                "accentGreen": Theme.accentGreen.toString(),
                "textMain": Theme.textMain.toString()
            };
            return JSON.stringify(t);
        }

        function getStatus(): string {
            var s = {
                "app": "OmaStudio",
                "activePhoto": mainWin.activePhotoPath,
                "theme": Theme.themeName,
                "themeMode": Theme.isDarkTheme ? "dark" : "light",
                "isProMode": mainWin.isProMode,
                "isSplitView": mainWin.isSplitView,
                "windowVisible": win.visible,
                "recipe": mainWin.buildRecipeObject()
            };
            return JSON.stringify(s);
        }
    }
}

