import QtQuick
import Quickshell
import qs.Commons
import qs.Ui

// OmaStudio in the Omarchy bar: shows or hides the editor window, and starts it when
// it isn't running. Drawn by the shell's own BarIconButton, so its size, colours,
// tooltip and press feedback follow the Omarchy theme and bar like the built-in
// widgets. The editor itself is the standalone omastudio-app.
BarWidget {
  id: root
  moduleName: "ozdil.omastudio"

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  function toggleEditor() {
    if (root.bar) root.bar.run("omastudio-dashboard")
    else Quickshell.execDetached(["omastudio-dashboard"])
  }

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    // Nerd Font camera-iris glyph, like the editor's own icon
    text: "󰄄"
    tooltipText: "OmaStudio"
    onPressed: function(mouseButton) { root.toggleEditor() }
  }
}
