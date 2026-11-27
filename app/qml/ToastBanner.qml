import QtQuick
import QtQuick.Layouts

// Auto-dismissing toast notification pill positioned over the viewport below the version strip.
Rectangle {
    id: banner

    property var host: null
    property Item anchorColumn: null
    property Item anchorStrip: null

    visible: banner.host !== null && banner.host.toastMessage !== "" && banner.anchorColumn !== null && banner.anchorStrip !== null
    // mapToItem isn't a binding dependency: the geometry it reads is listed so the
    // position follows layout changes and panel toggles
    x: {
        if (!banner.anchorColumn || !banner.host) return 0;
        [banner.anchorColumn.x, banner.anchorColumn.parent ? banner.anchorColumn.parent.x : 0, banner.anchorColumn.width, banner.width];
        return banner.anchorColumn.mapToItem(banner.host, (banner.anchorColumn.width - banner.width) / 2, 0).x;
    }
    y: {
        if (!banner.anchorColumn || !banner.anchorStrip || !banner.host) return 0;
        [banner.anchorColumn.y, banner.anchorColumn.parent ? banner.anchorColumn.parent.y : 0, banner.anchorStrip.y, banner.anchorStrip.height];
        return banner.anchorStrip.mapToItem(banner.host, 0, banner.anchorStrip.height + 8).y;
    }
    implicitWidth: banner.anchorColumn ? Math.min(banner.anchorColumn.width - 32, toastContent.implicitWidth + 28) : (toastContent.implicitWidth + 28)
    implicitHeight: Theme.sp(32)
    radius: Theme.r(16)
    color: Theme.bgDark
    border.color: banner.host ? banner.host.toastColor : Theme.accent
    border.width: 1
    clip: true
    z: 300

    RowLayout {
        id: toastContent
        anchors.centerIn: parent
        spacing: 8

        Text {
            text: banner.host ? banner.host.toastMessage : ""
            textFormat: Text.PlainText
            font.pixelSize: Theme.px(11)
            font.family: Theme.monoFont
            font.weight: Font.DemiBold
            color: banner.host ? banner.host.toastColor : Theme.accent
            elide: Text.ElideRight
            maximumLineCount: 1
            Layout.maximumWidth: banner.anchorColumn ? Math.max(0, banner.anchorColumn.width - 64) : 400
        }
    }
}
