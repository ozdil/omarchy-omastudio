import QtQuick

// Global Lightroom & Studio keyboard shortcuts routed to the main workspace.
// Single-key shortcuts are disabled while a text field has focus or a modal is open.
Item {
    id: router
    visible: false

    property var host: null
    property var viewport: null
    property var openFileDialog: null

    readonly property bool canShortcut: router.host !== null && !router.host.textInputFocused
    readonly property bool canSingleKey: router.canShortcut && !router.host.anyModalOpen

    Shortcut {
        sequence: "Ctrl+Z"
        enabled: router.canShortcut
        onActivated: router.host.undo()
    }
    Shortcut {
        sequences: ["Ctrl+Shift+Z", "Ctrl+Y"]
        enabled: router.canShortcut
        onActivated: router.host.redo()
    }
    Shortcut {
        sequence: "Ctrl+S"
        enabled: router.canShortcut
        onActivated: router.host.saveSidecar()
    }
    Shortcut {
        sequence: "Ctrl+Shift+C"
        enabled: router.canShortcut
        onActivated: router.host.copyRecipe()
    }
    Shortcut {
        sequence: "Ctrl+Shift+V"
        enabled: router.canShortcut
        onActivated: router.host.pasteRecipe()
    }
    Shortcut {
        sequence: "Ctrl+R"
        enabled: router.canShortcut
        onActivated: router.host.resetRecipe()
    }
    Shortcut {
        sequence: "C"
        enabled: router.canSingleKey
        onActivated: {
            if (!router.viewport) return;
            router.viewport.isCropMode = !router.viewport.isCropMode;
            router.host.showToast(router.viewport.isCropMode ? qsTr("Crop mode: on (C)") : qsTr("Crop mode: off"), Theme.accentYellow);
        }
    }
    Shortcut {
        sequence: "S"
        enabled: router.canSingleKey
        onActivated: {
            router.host.softProof = !router.host.softProof;
            router.host.showToast(router.host.softProof ? qsTr("Soft proof: %1").arg(router.host.proofProfileName()) : qsTr("Soft proof off"), Theme.accentYellow);
        }
    }
    Shortcut {
        sequence: "L"
        enabled: router.canSingleKey
        onActivated: if (router.viewport) router.viewport.loupeActive = !router.viewport.loupeActive
    }
    Shortcut {
        sequence: "Shift+F"
        enabled: router.canSingleKey
        onActivated: router.host.focusPeaking = !router.host.focusPeaking
    }
    Shortcut {
        sequence: "Shift+C"
        enabled: router.canSingleKey
        onActivated: router.host.compareMode = !router.host.compareMode
    }
    Shortcut {
        sequence: "Y"
        enabled: router.canSingleKey
        onActivated: {
            router.host.isSplitView = !router.host.isSplitView;
            router.host.requestRender();
        }
    }
    Shortcut {
        sequence: "Alt+1"
        onActivated: if (router.host) router.host.switchGradeVersion("A")
    }
    Shortcut {
        sequence: "Alt+2"
        onActivated: if (router.host) router.host.switchGradeVersion("B")
    }
    Shortcut {
        sequence: "Alt+3"
        onActivated: if (router.host) router.host.switchGradeVersion("C")
    }
    Shortcut {
        sequence: "Alt+4"
        onActivated: if (router.host) router.host.switchGradeVersion("D")
    }
    Shortcut {
        sequence: "Escape"
        onActivated: {
            if (!router.host) return;
            if (router.host.showAboutModal) router.host.showAboutModal = false;
            else if (router.host.showExportModal) router.host.showExportModal = false;
            else if (router.host.showCloudModal) router.host.showCloudModal = false;
            else if (router.viewport && router.viewport.isCropMode) router.viewport.isCropMode = false;
            else {
                var win = router.host.Window.window;
                if (win && win.activeFocusItem && win.activeFocusItem !== win.contentItem) {
                    win.contentItem.forceActiveFocus();
                }
            }
        }
    }

    // Lightroom Studio Culling & Ergonomics (1-5 Stars, Flags, Cinematic View)
    Shortcut {
        sequence: "1"
        enabled: router.canSingleKey
        onActivated: { router.host.photoRating = 1; router.host.showToast(qsTr("Rating: ★"), Theme.accentYellow); router.host.saveSidecar(true); }
    }
    Shortcut {
        sequence: "2"
        enabled: router.canSingleKey
        onActivated: { router.host.photoRating = 2; router.host.showToast(qsTr("Rating: ★★"), Theme.accentYellow); router.host.saveSidecar(true); }
    }
    Shortcut {
        sequence: "3"
        enabled: router.canSingleKey
        onActivated: { router.host.photoRating = 3; router.host.showToast(qsTr("Rating: ★★★"), Theme.accentYellow); router.host.saveSidecar(true); }
    }
    Shortcut {
        sequence: "4"
        enabled: router.canSingleKey
        onActivated: { router.host.photoRating = 4; router.host.showToast(qsTr("Rating: ★★★★"), Theme.accentYellow); router.host.saveSidecar(true); }
    }
    Shortcut {
        sequence: "5"
        enabled: router.canSingleKey
        onActivated: { router.host.photoRating = 5; router.host.showToast(qsTr("Rating: ★★★★★"), Theme.accentYellow); router.host.saveSidecar(true); }
    }
    Shortcut {
        sequence: "0"
        enabled: router.canSingleKey
        onActivated: { router.host.photoRating = 0; router.host.showToast(qsTr("Rating cleared"), Theme.textDim); router.host.saveSidecar(true); }
    }
    Shortcut {
        sequence: "X"
        enabled: router.canSingleKey
        onActivated: { router.host.photoFlag = (router.host.photoFlag === "reject" ? "none" : "reject"); router.host.showToast(router.host.flagMessage(router.host.photoFlag), Theme.highlightClip); router.host.saveSidecar(true); }
    }
    Shortcut {
        sequence: "P"
        enabled: router.canSingleKey
        onActivated: { router.host.photoFlag = (router.host.photoFlag === "pick" ? "none" : "pick"); router.host.showToast(router.host.flagMessage(router.host.photoFlag), Theme.accentGreen); router.host.saveSidecar(true); }
    }
    Shortcut {
        sequence: "U"
        enabled: router.canSingleKey
        onActivated: { router.host.photoFlag = "none"; router.host.showToast(qsTr("Flag cleared"), Theme.textDim); router.host.saveSidecar(true); }
    }
    Shortcut {
        sequence: "E"
        enabled: router.canSingleKey
        onActivated: {
            if (router.host.activePhotoPath === "" && !router.host.showExportModal) {
                router.host.showToast(qsTr("Open a photo first"), Theme.textDim);
                return;
            }
            router.host.showExportModal = !router.host.showExportModal;
        }
    }
    Shortcut {
        sequences: [StandardKey.Open]
        onActivated: if (router.openFileDialog) router.openFileDialog.open()
    }
    Shortcut {
        sequence: "Ctrl+0"
        onActivated: if (router.viewport) router.viewport.resetZoomFit()
    }
    Shortcut {
        sequence: "Ctrl+1"
        onActivated: if (router.viewport) router.viewport.setZoomAtPoint(router.viewport.width / 2, router.viewport.height / 2, router.viewport.zoomForPixelScale(1.0))
    }
    Shortcut {
        sequence: "Ctrl+Right"
        onActivated: if (router.host) router.host.stepPhoto(1)
    }
    Shortcut {
        sequence: "Ctrl+Left"
        onActivated: if (router.host) router.host.stepPhoto(-1)
    }

    // Keyboard access to the controls: Tab hides the panels (as in other photo editors),
    // so F6 / Shift+F6 move the focus from control to control; Space or Enter presses
    // the focused one and Escape gives the focus back to the photo
    Shortcut {
        sequence: "F6"
        onActivated: if (router.host) router.host.moveControlFocus(true)
    }
    Shortcut {
        sequence: "Shift+F6"
        onActivated: if (router.host) router.host.moveControlFocus(false)
    }

    Shortcut {
        sequence: "Tab"
        enabled: router.canSingleKey
        onActivated: {
            var anyOpen = router.host.showLeftPanel || router.host.showRightPanel || router.host.showFilmstrip;
            router.host.showLeftPanel = !anyOpen;
            router.host.showRightPanel = !anyOpen;
            router.host.showFilmstrip = !anyOpen;
        }
    }
}
