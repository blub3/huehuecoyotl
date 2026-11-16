pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Coyotl

// Add a folder on a network share as a library (SPEC 6): servers found on the network, the
// connection settings, a connection test that lists the share, and a folder picker.
// The password (or an SFTP key's passphrase) goes to the OS keystore; it is never stored in the
// library database. SFTP shows the server's host key fingerprint after connecting: adding the
// library trusts that key, and a different key is refused later.
FocusScope {
    id: page
    readonly property bool isPlayer: false

    property string protocol: "smb"
    property bool connected: false
    property bool busy: false
    property string status: ""
    property string currentPath: "/"
    property string parentPath: ""
    property var folders: []
    property var servers: []
    property bool searching: false
    property string hostKey: ""

    property var pending: ({ browse: 0, add: 0, discover: 0 })

    readonly property var protocols: [
        { key: "smb", label: "SMB" },
        { key: "nfs", label: "NFS" },
        { key: "webdav", label: "WebDAV" },
        { key: "ftp", label: "FTP" },
        { key: "sftp", label: "SFTP" }
    ]

    function shareLabel() {
        switch (protocol) {
        case "smb": return qsTr("Share")
        case "nfs": return qsTr("Export path")
        default: return qsTr("Base folder (optional)")
        }
    }

    // 2.0: "video" or "music".
    property string content: "video"

    function spec(path) {
        return {
            protocol: protocol,
            host: hostField.text.trim(),
            port: parseInt(portField.text) || 0,
            share: shareField.text.trim(),
            user: protocol === "nfs" ? "" : userField.text.trim(),
            tls: protocol === "webdav" && tlsBox.checked,
            key_file: protocol === "sftp" && keyField.text.trim() !== "" ? keyField.text.trim() : null,
            path: path || "",
            name: "",
            content: content
        }
    }

    function validate() {
        if (hostField.text.trim() === "") return qsTr("Please enter a server name or address.")
        if (protocol === "smb" && shareField.text.trim() === "") return qsTr("Please enter the name of the share.")
        if (protocol === "nfs" && shareField.text.trim() === "") return qsTr("Please enter the export path.")
        return ""
    }

    function browse(path) {
        const v = validate()
        if (v !== "") { status = v; return }
        busy = true
        status = qsTr("Connecting…")
        pending.browse = Core.browseNetwork(spec(), passwordField.text, path)
    }

    function addHere() {
        busy = true
        status = qsTr("Adding…")
        pending.add = Core.addNetworkLibrary(spec(currentPath), passwordField.text)
    }

    function discover() {
        searching = true
        pending.discover = Core.discover()
    }

    function useServer(s) {
        protocol = s.protocol
        hostField.text = s.host
        const def = { smb: 445, nfs: 2049, webdav: s.tls ? 443 : 80, ftp: 21, sftp: 22 }[protocol]
        portField.text = s.port && s.port !== def ? String(s.port) : ""
        tlsBox.checked = !!s.tls
        connected = false
        status = ""
        (protocol === "nfs" || protocol === "smb" ? shareField : userField).forceActiveFocus()
    }

    // Changing the settings invalidates the folder list.
    function settingsChanged() { connected = false; folders = []; currentPath = "/"; hostKey = "" }
    onProtocolChanged: settingsChanged()

    Component.onCompleted: discover()

    Connections {
        target: Core
        function onFolderListed(id, path, parent, entries) {
            if (id !== page.pending.browse) return
            page.busy = false
            page.connected = true
            page.hostKey = page.protocol === "sftp" ? Core.networkHostKey() : ""
            page.currentPath = path
            page.parentPath = parent
            page.folders = entries.filter(e => e.isDir)
            const videos = entries.length - page.folders.length
            page.status = videos > 0 ? qsTr("%n video(s) in this folder", "", videos) : ""
            Qt.callLater(() => folderList.forceActiveFocus())
        }
        function onRequestFailed(id, kind, message) {
            if (id === page.pending.discover) { page.searching = false; return }
            if (id !== page.pending.browse && id !== page.pending.add) return
            page.busy = false
            page.status = message
        }
        function onLibrariesListed(id, libs) {
            if (id !== page.pending.add) return
            page.busy = false
            Nav.back()
        }
        function onServersDiscovered(id, list) {
            if (id !== page.pending.discover) return
            page.searching = false
            page.servers = list
        }
    }

    onActiveFocusChanged: if (activeFocus && !hostField.activeFocus) Qt.callLater(() => (servers.length > 0 ? serverRow : protoRow).forceActiveFocus())

    Flickable {
        anchors.fill: parent
        contentHeight: col.implicitHeight + 64
        clip: true
        ScrollBar.vertical: ScrollBar {}

        ColumnLayout {
            id: col
            x: 32; y: 32
            width: Math.min(parent.width - 64, 880)
            spacing: 14

            Label { text: qsTr("Add network share"); color: Theme.text; font.pixelSize: Theme.fontTitle; font.bold: true }

            // ---- discovered servers
            RowLayout {
                spacing: 12
                Label { text: qsTr("Found on your network"); color: Theme.textDim; font.pixelSize: Theme.fontLarge; Layout.fillWidth: true }
                FocusButton {
                    id: searchAgain
                    text: page.searching ? qsTr("Searching…") : qsTr("Search again")
                    enabled: !page.searching
                    KeyNavigation.down: page.servers.length > 0 ? serverRow : protoRow
                    Keys.onLeftPressed: Nav.focusRail()
                    onClicked: page.discover()
                }
            }
            Label {
                visible: page.servers.length === 0
                text: page.searching ? qsTr("Looking for file servers…") : qsTr("No servers announced themselves. Enter the address below.")
                color: Theme.textDim
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }
            ListView {
                id: serverRow
                visible: page.servers.length > 0
                Layout.fillWidth: true
                Layout.preferredHeight: 56
                orientation: ListView.Horizontal
                spacing: 8
                clip: true
                model: page.servers
                keyNavigationEnabled: true
                KeyNavigation.up: searchAgain
                KeyNavigation.down: protoRow
                Keys.onLeftPressed: (e) => { if (currentIndex <= 0) Nav.focusRail(); else e.accepted = false }
                delegate: FocusButton {
                    required property var modelData
                    required property int index
                    focus: ListView.isCurrentItem
                    text: modelData.name + "  ·  " + modelData.protocol.toUpperCase()
                    tip: modelData.host
                    onClicked: page.useServer(modelData)
                }
            }

            // ---- connection settings
            Label { text: qsTr("Connection"); color: Theme.textDim; font.pixelSize: Theme.fontLarge; Layout.topMargin: 8 }
            Row {
                id: protoRow
                spacing: 8
                property int current: 0
                focus: false
                onActiveFocusChanged: if (activeFocus) protoRepeater.itemAt(Math.max(0, page.protocols.findIndex(p => p.key === page.protocol))).forceActiveFocus()
                Repeater {
                    id: protoRepeater
                    model: page.protocols
                    delegate: FocusButton {
                        required property var modelData
                        required property int index
                        text: modelData.label
                        primary: page.protocol === modelData.key
                        Accessible.role: Accessible.RadioButton
                        Accessible.checked: primary
                        KeyNavigation.left: index > 0 ? protoRepeater.itemAt(index - 1) : null
                        KeyNavigation.right: index < page.protocols.length - 1 ? protoRepeater.itemAt(index + 1) : null
                        KeyNavigation.up: page.servers.length > 0 ? serverRow : searchAgain
                        KeyNavigation.down: hostField
                        Keys.onLeftPressed: (e) => { if (index === 0) Nav.focusRail(); else e.accepted = false }
                        onClicked: page.protocol = modelData.key
                    }
                }
            }

            GridLayout {
                columns: 2
                columnSpacing: 12
                rowSpacing: 10
                Layout.fillWidth: true

                Label { text: qsTr("Server"); color: Theme.text }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8
                    TextField {
                        id: hostField
                        Layout.fillWidth: true
                        placeholderText: qsTr("Name or address, e.g. nas.local")
                        Accessible.name: qsTr("Server")
                        inputMethodHints: Qt.ImhNoAutoUppercase | Qt.ImhUrlCharactersOnly
                        KeyNavigation.up: protoRow
                        KeyNavigation.right: portField
                        KeyNavigation.down: shareField
                        onTextEdited: page.settingsChanged()
                        onAccepted: page.browse("/")
                    }
                    TextField {
                        id: portField
                        Layout.preferredWidth: 110
                        placeholderText: qsTr("Port")
                        Accessible.name: qsTr("Port")
                        inputMethodHints: Qt.ImhDigitsOnly
                        validator: IntValidator { bottom: 1; top: 65535 }
                        KeyNavigation.left: hostField
                        KeyNavigation.up: protoRow
                        KeyNavigation.down: shareField
                        onTextEdited: page.settingsChanged()
                        onAccepted: page.browse("/")
                    }
                }

                Label { text: page.shareLabel(); color: Theme.text }
                TextField {
                    id: shareField
                    Layout.fillWidth: true
                    placeholderText: page.protocol === "smb" ? qsTr("e.g. media") : page.protocol === "nfs" ? qsTr("e.g. /volume1/media")
                                   : page.protocol === "sftp" ? qsTr("e.g. /srv/media") : qsTr("e.g. /dav")
                    Accessible.name: page.shareLabel()
                    inputMethodHints: Qt.ImhNoAutoUppercase
                    KeyNavigation.up: hostField
                    KeyNavigation.down: page.protocol === "nfs" ? connectButton : userField
                    onTextEdited: page.settingsChanged()
                    onAccepted: page.browse("/")
                }

                Label { visible: page.protocol !== "nfs"; text: qsTr("User name"); color: Theme.text }
                TextField {
                    id: userField
                    visible: page.protocol !== "nfs"
                    Layout.fillWidth: true
                    placeholderText: page.protocol === "smb" ? qsTr("Empty for guest access") : page.protocol === "ftp" ? qsTr("Empty for anonymous") : ""
                    Accessible.name: qsTr("User name")
                    inputMethodHints: Qt.ImhNoAutoUppercase | Qt.ImhNoPredictiveText
                    KeyNavigation.up: shareField
                    KeyNavigation.down: passwordField
                    onTextEdited: page.settingsChanged()
                    onAccepted: passwordField.forceActiveFocus()
                }

                Label {
                    visible: page.protocol !== "nfs"
                    text: page.protocol === "sftp" && keyField.text.trim() !== "" ? qsTr("Key passphrase") : qsTr("Password")
                    color: Theme.text
                }
                TextField {
                    id: passwordField
                    visible: page.protocol !== "nfs"
                    Layout.fillWidth: true
                    echoMode: TextInput.Password
                    placeholderText: page.protocol === "sftp" && keyField.text.trim() !== "" ? qsTr("Empty if the key has none") : ""
                    Accessible.name: page.protocol === "sftp" && keyField.text.trim() !== "" ? qsTr("Key passphrase") : qsTr("Password")
                    inputMethodHints: Qt.ImhSensitiveData | Qt.ImhNoPredictiveText
                    KeyNavigation.up: userField
                    KeyNavigation.down: page.protocol === "webdav" ? tlsBox : page.protocol === "sftp" ? keyField : connectButton
                    onTextEdited: page.settingsChanged()
                    onAccepted: page.browse("/")
                }

                Label { visible: page.protocol === "sftp"; text: qsTr("Key file (optional)"); color: Theme.text }
                TextField {
                    id: keyField
                    visible: page.protocol === "sftp"
                    Layout.fillWidth: true
                    placeholderText: qsTr("Log in with a key instead of a password, e.g. ~/.ssh/id_ed25519")
                    Accessible.name: qsTr("Key file")
                    inputMethodHints: Qt.ImhNoAutoUppercase | Qt.ImhNoPredictiveText
                    KeyNavigation.up: passwordField
                    KeyNavigation.down: connectButton
                    onTextEdited: page.settingsChanged()
                    onAccepted: page.browse("/")
                }

                Item { visible: page.protocol === "webdav"; Layout.preferredWidth: 1 }
                CheckBox {
                    id: tlsBox
                    visible: page.protocol === "webdav"
                    text: qsTr("Use HTTPS")
                    KeyNavigation.up: passwordField
                    KeyNavigation.down: connectButton
                    onToggled: page.settingsChanged()
                    contentItem: Label { text: tlsBox.text; color: Theme.text; leftPadding: tlsBox.indicator.width + 8; verticalAlignment: Text.AlignVCenter }
                }
            }

            RowLayout {
                spacing: 12
                FocusButton {
                    id: connectButton
                    text: page.connected ? qsTr("Reconnect") : qsTr("Connect")
                    primary: !page.connected
                    enabled: !page.busy
                    KeyNavigation.up: page.protocol === "webdav" ? tlsBox : page.protocol === "nfs" ? shareField
                                    : page.protocol === "sftp" ? keyField : passwordField
                    KeyNavigation.down: page.connected ? folderList : null
                    KeyNavigation.right: page.connected ? addButton : null
                    Keys.onLeftPressed: Nav.focusRail()
                    onClicked: page.browse("/")
                }
                FocusButton {
                    id: addButton
                    visible: page.connected
                    primary: true
                    enabled: !page.busy
                    text: page.currentPath === "/" ? qsTr("Add the whole share") : qsTr("Add this folder")
                    KeyNavigation.left: connectButton
                    KeyNavigation.right: contentButton
                    KeyNavigation.down: folderList
                    onClicked: page.addHere()
                }
                // 2.0: what the folder holds; it decides how it is scanned.
                FocusButton {
                    id: contentButton
                    visible: page.connected
                    text: page.content === "music" ? qsTr("Contains: music") : qsTr("Contains: video")
                    tip: qsTr("Video folders become Movies and Shows, music folders Music")
                    KeyNavigation.left: addButton
                    KeyNavigation.down: folderList
                    onClicked: page.content = page.content === "music" ? "video" : "music"
                }
                Label {
                    text: page.status
                    color: Theme.textDim
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    Accessible.role: Accessible.StaticText
                    Accessible.name: text
                }
            }

            // ---- SFTP host key (trust on first use)
            Label {
                visible: page.connected && page.hostKey !== ""
                Layout.fillWidth: true
                wrapMode: Text.WrapAnywhere
                color: Theme.text
                text: qsTr("Server key fingerprint: %1").arg(page.hostKey)
                Accessible.role: Accessible.StaticText
                Accessible.name: text
            }
            Label {
                visible: page.connected && page.hostKey !== ""
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.textDim
                text: qsTr("Compare it with the server's own fingerprint before adding (on the server: ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub). Adding the library trusts this key; if the server later presents a different key, the connection is refused.")
            }

            // ---- folder picker
            Label {
                visible: page.connected
                text: qsTr("Folder: %1").arg(page.currentPath)
                color: Theme.textDim
                font.pixelSize: Theme.fontLarge
                Layout.topMargin: 6
                elide: Text.ElideMiddle
                Layout.fillWidth: true
            }
            ListView {
                id: folderList
                visible: page.connected
                Layout.fillWidth: true
                Layout.preferredHeight: contentHeight
                interactive: false
                spacing: 6
                model: (page.parentPath !== "" ? [{ name: "..", path: page.parentPath, up: true }] : []).concat(page.folders)
                keyNavigationEnabled: true
                onModelChanged: currentIndex = count > 0 ? 0 : -1
                Keys.onUpPressed: (e) => { if (currentIndex <= 0) addButton.forceActiveFocus(); else e.accepted = false }
                Keys.onLeftPressed: Nav.focusRail()
                delegate: ItemDelegate {
                    id: dirRow
                    required property var modelData
                    required property int index
                    width: ListView.view.width
                    height: 48
                    focus: ListView.isCurrentItem
                    focusPolicy: Qt.StrongFocus
                    Accessible.name: modelData.up ? qsTr("Up one folder") : modelData.name
                    function openIt() { page.browse(modelData.path) }
                    onClicked: openIt()
                    Keys.onReturnPressed: openIt()
                    Keys.onEnterPressed: openIt()
                    Keys.onRightPressed: openIt()
                    background: Rectangle {
                        radius: Theme.radius
                        color: dirRow.activeFocus || dirRow.hovered ? Theme.surfaceHi : Theme.surface
                        border.width: dirRow.activeFocus ? Theme.focusWidth : 0
                        border.color: Theme.focus
                    }
                    contentItem: RowLayout {
                        spacing: 14
                        Rectangle { Layout.preferredWidth: 26; Layout.preferredHeight: 20; radius: 4; color: Theme.textDim; opacity: 0.6 }
                        Label { text: dirRow.modelData.up ? qsTr("Up one folder") : dirRow.modelData.name; color: Theme.text; Layout.fillWidth: true; elide: Text.ElideRight }
                    }
                }
            }
            Label {
                visible: page.connected && page.folders.length === 0
                text: qsTr("No sub-folders here.")
                color: Theme.textDim
            }
        }
    }
}
