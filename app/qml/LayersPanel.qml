import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import OmaStudio.Host

Rectangle {
    id: root
    // Inside a panel section, which shows the title
    property bool embedded: false
    Layout.fillWidth: true
    implicitHeight: mainCol.implicitHeight + 20
    radius: Theme.radiusMd
    color: Theme.bgSurface
    border.color: Theme.border
    border.width: 1

    property var layersList: []
    property int selectedLayerIndex: -1
    // Deep copy: layer objects are shared with undo snapshots and grade versions,
    // so editing them in place silently rewrote history
    function cloneLayers() {
        return JSON.parse(JSON.stringify(root.layersList || []));
    }

    readonly property var activeL: (selectedLayerIndex >= 0 && selectedLayerIndex < layersList.length) ? layersList[selectedLayerIndex] : null

    // Which part of the active layer's mask is edited: -1 is the layer's own shape,
    // 0.. are its components
    property int editTarget: -1
    readonly property var activeComponents: activeL && activeL.components ? activeL.components : []
    readonly property var editShape: !activeL ? null
        : editTarget < 0 ? activeL.shape
        : (editTarget < activeComponents.length ? activeComponents[editTarget].shape : null)
    readonly property string editType: editShape ? editShape.type : ""

    // Brush settings for the next stroke
    property real brushSize: 0.05
    property real brushFeather: 0.5
    property real brushFlow: 1.0
    property bool brushErase: false
    property bool picking: false

    // What the photo overlay does for the shape being edited
    readonly property string overlayTool: !editShape ? ""
        : editType === "brush" ? "brush"
        : editType === "color_range" && picking ? "pick"
        : editType === "linear_gradient" ? "linear"
        : editType === "radial_gradient" ? "radial"
        : ""

    onSelectedLayerIndexChanged: { editTarget = -1; picking = false; }
    onEditTargetChanged: picking = false

    signal layersChanged()
    signal notify(string text, bool isError)

    property string enginePath: ""
    property string activePhotoPath: ""
    property string aiKind: ""

    function defaultShape(type) {
        if (type === "linear_gradient")
            return { "type": type, "start_x": 0.5, "start_y": 0.0, "end_x": 0.5, "end_y": 0.5 };
        if (type === "radial_gradient")
            return { "type": type, "center_x": 0.5, "center_y": 0.5, "radius_x": 0.25, "radius_y": 0.25, "feather": 0.5 };
        if (type === "luma_range")
            return { "type": type, "min_luma": 0.6, "max_luma": 1.0, "falloff": 0.1 };
        if (type === "color_range")
            return { "type": type, "r": 0.5, "g": 0.5, "b": 0.5, "range": 0.3, "feather": 0.5 };
        return { "type": "brush", "strokes": [], "aspect": 1.0 };
    }

    function typeLabel(type) {
        if (type === "linear_gradient") return qsTr("Linear");
        if (type === "radial_gradient") return qsTr("Radial");
        if (type === "luma_range") return qsTr("Luminance");
        if (type === "color_range") return qsTr("Colour");
        if (type === "brush") return qsTr("Brush");
        if (type === "bitmap") return qsTr("AI mask");
        return type;
    }

    function modeLabel(mode) {
        if (mode === "subtract") return qsTr("Subtract");
        if (mode === "intersect") return qsTr("Intersect");
        return qsTr("Add");
    }

    function newLayer(name, shape) {
        return {
            "id": "layer_" + Date.now(),
            "name": name,
            "enabled": true,
            "invert": false,
            "opacity": 1.0,
            "shape": shape,
            "components": [],
            "exposure": 0.0, "contrast": 0.0, "highlights": 0.0, "shadows": 0.0,
            "whites": 0.0, "blacks": 0.0, "clarity": 0.0, "saturation": 0.0,
            "tint_r": 0.0, "tint_g": 0.0, "tint_b": 0.0
        };
    }

    function addLayer(type) {
        var list = root.cloneLayers();
        var n = list.length + 1;
        var name = type === "linear_gradient" ? qsTr("Linear Gradient %1").arg(n)
                 : type === "radial_gradient" ? qsTr("Radial Mask %1").arg(n)
                 : type === "luma_range" ? qsTr("Luma Range %1").arg(n)
                 : type === "color_range" ? qsTr("Colour Range %1").arg(n)
                 : qsTr("Brush %1").arg(n);
        list.push(root.newLayer(name, root.defaultShape(type)));
        root.layersList = list;
        root.selectedLayerIndex = list.length - 1;
        root.editTarget = -1;
        // A new colour range starts by picking its colour
        root.picking = type === "color_range";
        root.layersChanged();
    }

    // Applies fn to a copy of the active layer and publishes the result
    function editLayer(fn) {
        if (!root.activeL) return;
        var list = root.cloneLayers();
        var l = list[root.selectedLayerIndex];
        if (!l.components) l.components = [];
        fn(l);
        root.layersList = list;
        root.layersChanged();
    }

    function setLayerValue(key, v) {
        editLayer(function(l) { l[key] = v; });
    }

    function setEditShape(shape) {
        var target = root.editTarget;
        editLayer(function(l) {
            if (target < 0) l.shape = shape;
            else if (target < l.components.length) l.components[target].shape = shape;
        });
    }

    function setShapeValue(key, v) {
        if (!root.editShape) return;
        var s = JSON.parse(JSON.stringify(root.editShape));
        s[key] = v;
        root.setEditShape(s);
    }

    function addStroke(points, aspect) {
        if (root.editType !== "brush" || !points || points.length === 0) return;
        var s = JSON.parse(JSON.stringify(root.editShape));
        s.strokes = (s.strokes || []).concat([{
            "points": points, "size": root.brushSize, "feather": root.brushFeather,
            "flow": root.brushFlow, "erase": root.brushErase
        }]);
        s.aspect = aspect;
        root.setEditShape(s);
    }

    function setPickedColour(r, g, b) {
        if (root.editType !== "color_range") return;
        var s = JSON.parse(JSON.stringify(root.editShape));
        s.r = r; s.g = g; s.b = b;
        root.picking = false;
        root.setEditShape(s);
    }

    function addComponent(type) {
        var count = root.activeComponents.length;
        editLayer(function(l) {
            l.components.push({ "mode": "add", "invert": false, "shape": root.defaultShape(type) });
        });
        root.editTarget = count;
        root.picking = type === "color_range";
    }

    // Local segmentation model (ONNX Runtime on the NPU, GPU or CPU) makes a mask image
    function addAiLayer(kind) {
        if (root.enginePath === "" || root.activePhotoPath === "" || aiProc.running) return;
        root.aiKind = kind;
        aiProc.command = [root.enginePath, "ai-mask", root.activePhotoPath, "--kind", kind];
        aiProc.running = true;
    }

    Process {
        id: aiProc
        onFinished: function(exitCode, output) {
            var resp;
            try { resp = JSON.parse(String(output || "")); } catch (e) { resp = { "success": false, "error": qsTr("The engine gave no answer") }; }
            if (!resp.success) {
                root.notify(String(resp.error || qsTr("AI mask failed")).split("\n")[0], true);
                return;
            }
            var list = root.cloneLayers();
            var label = root.aiKind === "sky" ? qsTr("AI Sky") : qsTr("AI Subject");
            list.push(root.newLayer(qsTr("%1 %2").arg(label).arg(list.length + 1), { "type": "bitmap", "path": resp.data.path }));
            root.layersList = list;
            root.selectedLayerIndex = list.length - 1;
            root.layersChanged();
            root.notify(qsTr("%1 mask ready (%2)").arg(label).arg(resp.data.provider), false);
        }
    }

    component SmallButton: Rectangle {
        id: sb
        property string label: ""
        property color labelColor: Theme.textMain
        property bool active: false
        signal clicked()
        implicitWidth: sbText.implicitWidth + 12
        implicitHeight: 20
        radius: Theme.r(4)
        opacity: enabled ? 1.0 : 0.4
        color: active ? Theme.bgCardHover : (sbMouse.containsMouse ? Theme.bgCardHover : Theme.bgCard)
        border.color: active ? sb.labelColor : Theme.border
        border.width: 1
        Text {
            id: sbText
            anchors.centerIn: parent
            width: Math.min(implicitWidth, sb.width - 8)
            elide: Text.ElideRight
            text: sb.label
            textFormat: Text.PlainText
            font.family: Theme.monoFont
            font.pixelSize: Theme.px(8)
            font.weight: Font.Bold
            color: sb.labelColor
        }
        MouseArea {
            id: sbMouse
            anchors.fill: parent
            cursorShape: Qt.PointingHandCursor
            hoverEnabled: true
            onClicked: sb.clicked()
        }
    }

    component AiButton: SmallButton {
        id: ab
        property string kind: ""
        enabled: root.activePhotoPath !== "" && !aiProc.running
        labelColor: Theme.accentGreen
        label: aiProc.running && root.aiKind === ab.kind ? qsTr("Detecting…") : ab.text
        property string text: ""
        onClicked: root.addAiLayer(ab.kind)
    }

    component InvertToggle: Rectangle {
        id: it
        property bool on: false
        signal clicked()
        implicitWidth: 16
        implicitHeight: 16
        radius: Theme.r(3)
        color: it.on ? Theme.accentCyan : "transparent"
        border.color: Theme.border
        border.width: 1
        Text {
            anchors.centerIn: parent
            text: qsTr("I", "invert mask toggle, first letter of Invert")
            textFormat: Text.PlainText
            font.family: Theme.monoFont
            font.pixelSize: Theme.px(8)
            font.weight: Font.Bold
            color: it.on ? "#ffffff" : Theme.textDim
        }
        MouseArea {
            anchors.fill: parent
            cursorShape: Qt.PointingHandCursor
            onClicked: it.clicked()
        }
    }

    component DeleteButton: Rectangle {
        id: db
        signal clicked()
        implicitWidth: 16
        implicitHeight: 16
        radius: Theme.r(3)
        color: "transparent"
        Text {
            anchors.centerIn: parent
            text: "X"
            textFormat: Text.PlainText
            font.family: Theme.monoFont
            font.pixelSize: Theme.px(9)
            font.weight: Font.Bold
            color: Theme.accentRed
        }
        MouseArea {
            anchors.fill: parent
            cursorShape: Qt.PointingHandCursor
            onClicked: db.clicked()
        }
    }

    component Hint: Text {
        Layout.fillWidth: true
        textFormat: Text.PlainText
        wrapMode: Text.WordWrap
        font.family: Theme.fontFamily
        font.pixelSize: Theme.px(10)
        color: Theme.textDim
    }

    component LayerSlider: SliderGroup {
        property string key: ""
        from: -100.0
        to: 100.0
        value: root.activeL && root.activeL[key] !== undefined ? root.activeL[key] : defaultValue
        onSliderMoved: function(v) { root.setLayerValue(key, v) }
    }

    ColumnLayout {
        id: mainCol
        anchors.fill: parent
        anchors.margins: 10
        spacing: 10

        Text {
            visible: !root.embedded
            text: qsTr("Masks")
            textFormat: Text.PlainText
            font.family: Theme.fontFamily
            font.pixelSize: Theme.px(10)
            font.weight: Font.DemiBold
            font.letterSpacing: 1
            color: Theme.textDim
        }

        // New layer with a mask of each kind
        Flow {
            Layout.fillWidth: true
            spacing: 6
            SmallButton { label: qsTr("+ Linear"); labelColor: Theme.accentCyan; onClicked: root.addLayer("linear_gradient") }
            SmallButton { label: qsTr("+ Radial"); labelColor: Theme.accentOrange; onClicked: root.addLayer("radial_gradient") }
            SmallButton { label: qsTr("+ Brush"); labelColor: Theme.accentMagenta; onClicked: root.addLayer("brush") }
            SmallButton { label: qsTr("+ Luminance"); labelColor: Theme.accentYellow; onClicked: root.addLayer("luma_range") }
            SmallButton { label: qsTr("+ Colour"); labelColor: Theme.accentGreen; onClicked: root.addLayer("color_range") }
        }

        // Masks from local AI models; wraps when the labels are long
        Flow {
            Layout.fillWidth: true
            spacing: 6
            AiButton { text: "+ " + qsTr("AI Subject"); kind: "subject" }
            AiButton { text: "+ " + qsTr("AI Sky"); kind: "sky" }
        }

        // Layers List View
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 4
            visible: root.layersList.length > 0

            Repeater {
                model: root.layersList.length

                delegate: Rectangle {
                    id: layerItem
                    Layout.fillWidth: true
                    implicitHeight: 28
                    radius: Theme.r(4)
                    color: root.selectedLayerIndex === index ? Theme.bgCardHover : Theme.bgCard
                    border.color: root.selectedLayerIndex === index ? Theme.accent : Theme.border
                    border.width: 1

                    readonly property var curLayer: root.layersList[index]

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 8
                        anchors.rightMargin: 8
                        spacing: 8

                        CustomSwitch {
                            checked: layerItem.curLayer ? layerItem.curLayer.enabled : false
                            activeColor: Theme.accent
                            onToggled: function(c) {
                                var list = root.cloneLayers();
                                list[index].enabled = c;
                                root.layersList = list;
                                root.layersChanged();
                            }
                        }

                        Text {
                            text: layerItem.curLayer ? layerItem.curLayer.name : ""
                            textFormat: Text.PlainText
                            font.family: Theme.fontFamily
                            font.pixelSize: Theme.px(11)
                            font.weight: root.selectedLayerIndex === index ? Font.Bold : Font.Medium
                            color: layerItem.curLayer && layerItem.curLayer.enabled ? Theme.textMain : Theme.textDim
                            Layout.fillWidth: true
                            elide: Text.ElideRight
                        }

                        InvertToggle {
                            on: layerItem.curLayer ? !!layerItem.curLayer.invert : false
                            onClicked: {
                                var list = root.cloneLayers();
                                list[index].invert = !list[index].invert;
                                root.layersList = list;
                                root.layersChanged();
                            }
                        }

                        DeleteButton {
                            onClicked: {
                                var list = root.cloneLayers();
                                list.splice(index, 1);
                                root.layersList = list;
                                // Keep the same layer selected when one above it is removed
                                if (index < root.selectedLayerIndex) {
                                    root.selectedLayerIndex--;
                                } else if (root.selectedLayerIndex >= list.length) {
                                    root.selectedLayerIndex = list.length - 1;
                                }
                                root.layersChanged();
                            }
                        }
                    }

                    MouseArea {
                        anchors.fill: parent
                        propagateComposedEvents: true
                        z: -1
                        onClicked: root.selectedLayerIndex = index
                    }
                }
            }
        }

        // Active layer: its mask, then its adjustments
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 8
            visible: root.activeL !== null

            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }

            Text {
                text: root.activeL ? root.activeL.name : ""
                textFormat: Text.PlainText
                font.family: Theme.fontFamily
                font.pixelSize: Theme.px(9)
                font.weight: Font.Bold
                color: Theme.accent
            }

            // The layer's shape and the components combined with it
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 4

                Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: 24
                    radius: Theme.r(4)
                    color: root.editTarget < 0 ? Theme.bgCardHover : Theme.bgCard
                    border.color: root.editTarget < 0 ? Theme.accent : Theme.border
                    border.width: 1
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        anchors.left: parent.left
                        anchors.leftMargin: 8
                        text: root.activeL ? root.typeLabel(root.activeL.shape.type) : ""
                        textFormat: Text.PlainText
                        font.family: Theme.fontFamily
                        font.pixelSize: Theme.px(10)
                        color: Theme.textMain
                    }
                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.editTarget = -1
                    }
                }

                Repeater {
                    model: root.activeComponents.length
                    delegate: Rectangle {
                        id: compItem
                        required property int index
                        readonly property var comp: root.activeComponents[index]
                        Layout.fillWidth: true
                        implicitHeight: 24
                        radius: Theme.r(4)
                        color: root.editTarget === index ? Theme.bgCardHover : Theme.bgCard
                        border.color: root.editTarget === index ? Theme.accent : Theme.border
                        border.width: 1

                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.editTarget = compItem.index
                        }

                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 6
                            anchors.rightMargin: 8
                            spacing: 6

                            // Click cycles add, subtract, intersect
                            SmallButton {
                                implicitWidth: Math.max(implicitHeight * 3.5, implicitWidth)
                                label: compItem.comp ? root.modeLabel(compItem.comp.mode) : ""
                                labelColor: compItem.comp && compItem.comp.mode === "subtract" ? Theme.accentRed
                                          : compItem.comp && compItem.comp.mode === "intersect" ? Theme.accentCyan
                                          : Theme.accentGreen
                                onClicked: {
                                    var i = compItem.index;
                                    root.editLayer(function(l) {
                                        var m = l.components[i].mode;
                                        l.components[i].mode = m === "add" || !m ? "subtract" : m === "subtract" ? "intersect" : "add";
                                    });
                                }
                            }
                            Text {
                                Layout.fillWidth: true
                                text: compItem.comp ? root.typeLabel(compItem.comp.shape.type) : ""
                                textFormat: Text.PlainText
                                elide: Text.ElideRight
                                font.family: Theme.fontFamily
                                font.pixelSize: Theme.px(10)
                                color: Theme.textMain
                            }
                            InvertToggle {
                                on: compItem.comp ? !!compItem.comp.invert : false
                                onClicked: {
                                    var i = compItem.index;
                                    root.editLayer(function(l) { l.components[i].invert = !l.components[i].invert; });
                                }
                            }
                            DeleteButton {
                                onClicked: {
                                    var i = compItem.index;
                                    if (root.editTarget >= i) root.editTarget = root.editTarget === i ? -1 : root.editTarget - 1;
                                    root.editLayer(function(l) { l.components.splice(i, 1); });
                                }
                            }
                        }
                    }
                }

                // Further shapes combined with the layer's mask
                Flow {
                    Layout.fillWidth: true
                    spacing: 6
                    Text {
                        height: 20
                        verticalAlignment: Text.AlignVCenter
                        text: qsTr("Combine with:")
                        textFormat: Text.PlainText
                        font.family: Theme.fontFamily
                        font.pixelSize: Theme.px(10)
                        color: Theme.textDim
                    }
                    SmallButton { label: qsTr("Brush"); onClicked: root.addComponent("brush") }
                    SmallButton { label: qsTr("Colour"); onClicked: root.addComponent("color_range") }
                    SmallButton { label: qsTr("Luminance"); onClicked: root.addComponent("luma_range") }
                    SmallButton { label: qsTr("Linear"); onClicked: root.addComponent("linear_gradient") }
                    SmallButton { label: qsTr("Radial"); onClicked: root.addComponent("radial_gradient") }
                }
            }

            // Settings of the selected mask part
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 6

                Hint {
                    visible: root.editType === "linear_gradient"
                    text: qsTr("Drag the handles on the photo. The effect builds up from the hollow handle to the solid one.")
                }

                Hint {
                    visible: root.editType === "radial_gradient"
                    text: qsTr("Drag the centre or the edge handles on the photo.")
                }
                SliderGroup {
                    visible: root.editType === "radial_gradient"
                    title: qsTr("Feather")
                    from: 0; to: 100; defaultValue: 50
                    value: root.editType === "radial_gradient" ? root.editShape.feather * 100 : 50
                    onSliderMoved: function(v) { root.setShapeValue("feather", v / 100) }
                }

                SliderGroup {
                    visible: root.editType === "luma_range"
                    title: qsTr("Darkest")
                    from: 0; to: 100; defaultValue: 60
                    value: root.editType === "luma_range" ? root.editShape.min_luma * 100 : 60
                    onSliderMoved: function(v) { root.setShapeValue("min_luma", Math.min(v / 100, root.editShape.max_luma)) }
                }
                SliderGroup {
                    visible: root.editType === "luma_range"
                    title: qsTr("Brightest")
                    from: 0; to: 100; defaultValue: 100
                    value: root.editType === "luma_range" ? root.editShape.max_luma * 100 : 100
                    onSliderMoved: function(v) { root.setShapeValue("max_luma", Math.max(v / 100, root.editShape.min_luma)) }
                }
                SliderGroup {
                    visible: root.editType === "luma_range"
                    title: qsTr("Falloff")
                    from: 0; to: 50; defaultValue: 10
                    value: root.editType === "luma_range" ? root.editShape.falloff * 100 : 10
                    onSliderMoved: function(v) { root.setShapeValue("falloff", v / 100) }
                }

                RowLayout {
                    visible: root.editType === "color_range"
                    Layout.fillWidth: true
                    spacing: 8
                    Rectangle {
                        implicitWidth: 28
                        implicitHeight: 20
                        radius: Theme.r(4)
                        border.color: Theme.border
                        color: root.editType === "color_range" ? Qt.rgba(root.editShape.r, root.editShape.g, root.editShape.b, 1) : "transparent"
                    }
                    SmallButton {
                        label: root.picking ? qsTr("Click the photo…") : qsTr("Pick colour")
                        active: root.picking
                        labelColor: Theme.accentGreen
                        onClicked: root.picking = !root.picking
                    }
                    Item { Layout.fillWidth: true }
                }
                SliderGroup {
                    visible: root.editType === "color_range"
                    title: qsTr("Range")
                    from: 0; to: 100; defaultValue: 30
                    value: root.editType === "color_range" ? root.editShape.range * 100 : 30
                    onSliderMoved: function(v) { root.setShapeValue("range", v / 100) }
                }
                SliderGroup {
                    visible: root.editType === "color_range"
                    title: qsTr("Feather")
                    from: 0; to: 100; defaultValue: 50
                    value: root.editType === "color_range" ? root.editShape.feather * 100 : 50
                    onSliderMoved: function(v) { root.setShapeValue("feather", v / 100) }
                }

                Hint {
                    visible: root.editType === "brush"
                    text: qsTr("Paint on the photo. The middle mouse button pans.")
                }
                RowLayout {
                    visible: root.editType === "brush"
                    Layout.fillWidth: true
                    spacing: 6
                    SmallButton {
                        label: qsTr("Paint")
                        active: !root.brushErase
                        labelColor: Theme.accentMagenta
                        onClicked: root.brushErase = false
                    }
                    SmallButton {
                        label: qsTr("Erase")
                        active: root.brushErase
                        labelColor: Theme.textMain
                        onClicked: root.brushErase = true
                    }
                    Item { Layout.fillWidth: true }
                    SmallButton {
                        label: qsTr("Clear")
                        enabled: root.editType === "brush" && (root.editShape.strokes || []).length > 0
                        labelColor: Theme.accentRed
                        onClicked: root.setShapeValue("strokes", [])
                    }
                }
                SliderGroup {
                    visible: root.editType === "brush"
                    title: qsTr("Size")
                    from: 0.5; to: 30; defaultValue: 5; stepSize: 0.5; decimals: 1; suffix: " %"
                    value: root.brushSize * 100
                    onSliderMoved: function(v) { root.brushSize = v / 100 }
                }
                SliderGroup {
                    visible: root.editType === "brush"
                    title: qsTr("Feather")
                    from: 0; to: 100; defaultValue: 50
                    value: root.brushFeather * 100
                    onSliderMoved: function(v) { root.brushFeather = v / 100 }
                }
                SliderGroup {
                    visible: root.editType === "brush"
                    title: qsTr("Flow")
                    from: 1; to: 100; defaultValue: 100
                    value: root.brushFlow * 100
                    onSliderMoved: function(v) { root.brushFlow = v / 100 }
                }
            }

            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.border }

            LayerSlider {
                key: "opacity"
                title: qsTr("Layer Opacity")
                from: 0.0; to: 1.0; defaultValue: 1.0
                stepSize: 0.05; decimals: 2
                accentColor: Theme.accentCyan
            }
            LayerSlider {
                key: "exposure"
                title: qsTr("Target Exposure")
                from: -4.0; to: 4.0
                stepSize: 0.1; decimals: 1; suffix: " EV"
                accentColor: Theme.accentYellow
            }
            LayerSlider { key: "contrast"; title: qsTr("Target Contrast") }
            LayerSlider { key: "highlights"; title: qsTr("Highlights") }
            LayerSlider { key: "shadows"; title: qsTr("Shadows") }
            LayerSlider { key: "whites"; title: qsTr("Whites") }
            LayerSlider { key: "blacks"; title: qsTr("Blacks") }
            LayerSlider { key: "saturation"; title: qsTr("Target Saturation"); accentColor: Theme.accentGreen }
        }
    }
}
