import QtQuick
import QtQuick.Window
import OmaStudio.Host

// The application window. Calls on the control socket (`omastudio ipc <method>`,
// Omarchy agents, a second launch) arrive through Host.ipcCall and are answered
// with a line of text.
Window {
    id: win
    title: mainWin.activePhotoPath !== "" ? mainWin.activePhotoPath.split("/").pop() + " - OmaStudio" : "OmaStudio"
    width: 1440
    height: 920
    minimumWidth: 1024
    minimumHeight: 680
    visible: true
    color: Theme.bgBase

    MainWindow {
        id: mainWin
        anchors.fill: parent
        // Arabic, Hebrew and Persian lay the interface out right to left
        LayoutMirroring.enabled: I18n.rightToLeft
        LayoutMirroring.childrenInherit: true
    }

    // Loads the translation for the stored or system language
    Component.onCompleted: I18n.apply()

    function present() {
        win.show();
        win.raise();
        win.requestActivate();
    }

    Connections {
        target: Host
        function onIpcCall(id, method, args) {
            var result;
            try {
                result = ipc.dispatch(method, JSON.parse(args));
            } catch (e) {
                result = "Error: " + e;
            }
            Host.ipcReply(id, typeof result === "string" ? result : JSON.stringify(result));
        }
    }

    QtObject {
        id: ipc

        function str(v) {
            return (v === undefined || v === null) ? "" : String(v);
        }

        function dispatch(method, a) {
            switch (method) {
            case "show":
                win.present();
                return "OK";
            case "toggle":
                if (win.visible) win.hide(); else win.present();
                return win.visible;
            case "openPhoto":
                if (str(a[0]).length === 0) return "Error: empty photo path";
                mainWin.loadPhoto(str(a[0]));
                win.present();
                return "OK";
            case "applyRecipe":
                if (str(a[0]).length === 0) return "Error: empty recipe json";
                mainWin.applyRecipeObject(typeof a[0] === "object" ? a[0] : JSON.parse(a[0]));
                return "OK";
            case "optimizeSocial":
                if (str(a[0]).length === 0) return "Error: empty platform code";
                mainWin.triggerSocial(str(a[0]));
                return "OK";
            case "applyPreset":
                if (str(a[0]).length === 0) return "Error: empty preset name";
                mainWin.applyPresetNamed(str(a[0]));
                return "OK";
            case "setExposure":
                mainWin.setExposureEv(Number(a[0]));
                return "OK";
            case "setWarmth":
                mainWin.setWarmthKelvin(Number(a[0]));
                return "OK";
            case "reset":
                mainWin.resetRecipe();
                return "OK";
            case "showExport":
                if (mainWin.activePhotoPath === "") return "Error: no photo open";
                mainWin.showExportModal = true;
                return "OK";
            case "showCloud":
                mainWin.refreshCloud();
                mainWin.showCloudModal = true;
                win.present();
                return "OK";
            case "cloudAccounts":
                return mainWin.cloudAccounts;
            case "openCloud":
                // openCloud <account> [folder]: browse a connected account in the filmstrip
                if (str(a[0]).length === 0) return "Error: empty account";
                mainWin.openCloudFolder(str(a[0]), str(a[1]));
                return "OK";
            case "setRightTab":
                if (["adjust", "masks", "looks", "ai"].indexOf(str(a[0])) < 0) return "Error: unknown tab";
                mainWin.rightTab = str(a[0]);
                return "OK";
            case "toggleCrop":
                return mainWin.toggleCropMode();
            case "toggleSplit":
                return mainWin.toggleSplitView();
            case "applyLut":
                if (str(a[0]).length === 0) return "Error: empty lut name";
                mainWin.activeLutName = str(a[0]);
                mainWin.activeLutIntensity = (a.length > 1 && Number(a[1]) >= 0) ? Number(a[1]) : 1.0;
                mainWin.activeLutPath = "";
                mainWin.requestRender();
                return "OK";
            case "switchGradeVersion":
                if (str(a[0]).length === 0) return "Error: empty version";
                mainWin.switchGradeVersion(str(a[0]));
                return "OK";
            case "screenshot":
                // Saves the window as PNG (written shortly after the answer)
                if (str(a[0]).length === 0) return "Error: empty output path";
                var out = str(a[0]);
                if (!win.contentItem.grabToImage(function(r) { r.saveToFile(out); })) return "Error: window not visible";
                return "OK";
            case "applyCameraRecipe":
                return mainWin.applyCameraRecipe(str(a[0]));
            case "fujiRecipes":
                return mainWin.fujiRecipeLibrary();
            case "applyFujiRecipe":
                if (str(a[0]).length === 0) return "Error: empty recipe name";
                return mainWin.applyFujiRecipe(str(a[0]));
            case "aiEdit":
                return mainWin.aiEdit(str(a[0]));
            case "aiMask":
                return mainWin.aiMask(str(a[0]));
            case "importXmp":
                return mainWin.importXmp();
            case "writeXmp":
                return mainWin.writeXmp(str(a[0]) === "force");
            case "setZoom":
                return mainWin.setViewZoom(a.length > 0 ? a[0] : "fit");
            case "reloadTheme":
                return Theme.reloadTheme();
            case "getTheme":
                return {
                    "name": Theme.themeName,
                    "isDark": Theme.isDarkTheme,
                    "bgBase": Theme.bgBase.toString(),
                    "bgDark": Theme.bgDark.toString(),
                    "bgSurface": Theme.bgSurface.toString(),
                    "accent": Theme.accent.toString(),
                    "accentGreen": Theme.accentGreen.toString(),
                    "textMain": Theme.textMain.toString()
                };
            case "getStatus":
                return {
                    "app": "OmaStudio",
                    "version": Host.version,
                    "activePhoto": mainWin.activePhotoPath,
                    "theme": Theme.themeName,
                    "themeMode": Theme.isDarkTheme ? "dark" : "light",
                    "isProMode": mainWin.isProMode,
                    "isSplitView": mainWin.isSplitView,
                    "windowVisible": win.visible,
                    "devicePixelRatio": win.screen ? win.screen.devicePixelRatio : 1,
                    "view": mainWin.viewState(),
                    "recipe": mainWin.buildRecipeObject()
                };
            default:
                return "Error: unknown method " + method;
            }
        }
    }
}
