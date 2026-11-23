pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Issue #121: adding a network share in three steps.
//  1 Server: the servers found on the network (a note on those already added), or a protocol
//    and a server typed by hand.
//  2 Sign in: user and password (SFTP: or a key; empty for a guest); an SMB server then lists
//    its shares, NFS asks for the export path, the others take an optional base folder. The
//    connection is tested here. SFTP shows the server's key fingerprint to trust.
//  3 Folders: ShareFolderBrowser; the share is created with the first folders added, so
//    leaving earlier keeps nothing, no password either.
// A server and share already in the library leads to that share's "Add folders" instead
// (`openExisting`). `backend` is the core or a test stub: discover(), shares(),
// shareSignIn(spec, password), shareBrowse(...), shareCreate(...).
FocusScope {
    id: flow
    property var backend: null
    signal openExisting(var share, string name)
    signal finished()
    signal closed()

    property int step: 1
    property string protocol: "smb"
    property string host: ""
    property int port: 0
    property bool tls: false
    property var servers: []
    property bool searching: false
    property var known: []          // Core.shares().shares
    property var smbShares: []      // step 2 on an SMB server: [{name, existing}]
    property bool typeShare: false  // the server did not list its shares: type one
    property string share: ""       // SMB share, NFS export, base folder of the others
    property string hostKey: ""
    property bool busy: false
    property string status: ""
    property var pending: ({ discover: 0, shares: 0, signIn: 0 })

    readonly property var protocols: [
        { key: "smb", label: "SMB" }, { key: "nfs", label: "NFS" }, { key: "webdav", label: "WebDAV" },
        { key: "ftp", label: "FTP" }, { key: "sftp", label: "SFTP" }
    ]
    readonly property string protocolLabel: (protocols.find(p => p.key === protocol) || { label: "" }).label

    function spec() {
        return {
            protocol: protocol,
            host: host.trim(),
            port: port,
            share: share.trim(),
            user: protocol === "nfs" ? "" : userField.text.trim(),
            tls: protocol === "webdav" && tls,
            key_file: protocol === "sftp" && keyField.text.trim() !== "" ? keyField.text.trim() : null,
            path: "",
            name: "",
            content: "video"
        }
    }
    function password() { return protocol === "nfs" ? "" : passwordField.text }
    function knownName(id) {
        const s = known.find(k => k.id === id)
        return s ? s.name : ""
    }
    function alreadyAdded(server) {
        const h = (server.host || "").toLowerCase()
        return known.some(k => k.protocol === server.protocol && (k.host || "").toLowerCase() === h)
    }

    function goTo(n) {
        step = n
        status = ""
        busy = false
        if (n <= 2) { hostKey = "" }
        if (n === 1) { smbShares = []; typeShare = false; share = "" }
        if (n === 3) { folders.chosen = []; folders.browse("/") }
        Qt.callLater(focusStep)
    }
    function focusStep() {
        if (step === 1) (servers.length > 0 ? serverList : protoRow).forceActiveFocus()
        else if (step === 2) (protocol === "nfs" ? shareField : userField).forceActiveFocus()
        else folders.focusFirst()
    }
    // Up from the top of a step: the step back button, else on to the page (its Back button).
    function upFromTop(e) {
        if (backButton.visible) backButton.forceActiveFocus()
        else e.accepted = false
    }
    function back() {
        if (step === 1) flow.closed()
        else goTo(step - 1)
    }

    // Step 1
    function useServer(s) {
        protocol = s.protocol
        host = s.host
        const def = { smb: 445, nfs: 2049, webdav: s.tls ? 443 : 80, ftp: 21, sftp: 22 }[s.protocol]
        port = s.port && s.port !== def ? s.port : 0
        tls = !!s.tls
        goTo(2)
    }
    function next() {
        if (hostField.text.trim() === "") { status = qsTr("Please enter a server name or address."); return }
        host = hostField.text.trim()
        port = parseInt(portField.text) || 0
        tls = tlsBox.checked
        goTo(2)
    }

    // Step 2
    function signIn() {
        if (protocol === "nfs" && share.trim() === "") { status = qsTr("Please enter the export path."); return }
        if (protocol === "smb" && typeShare && share.trim() === "") { status = qsTr("Please enter the name of the share."); return }
        busy = true
        status = qsTr("Connecting…")
        pending.signIn = backend.shareSignIn(spec(), password())
    }
    function chooseShare(s) {
        if (s.existing !== null && s.existing !== undefined) { flow.openExisting(s.existing, knownName(s.existing)); return }
        share = s.name
        signIn()
    }
    function signedIn(d) {
        busy = false
        if (d.shares !== undefined) {
            smbShares = d.shares
            status = d.shares.length === 0 ? qsTr("This server offers no shares to this user.") : ""
            Qt.callLater(() => (d.shares.length > 0 ? shareList : userField).forceActiveFocus())
            return
        }
        if (d.existing !== null && d.existing !== undefined) { flow.openExisting(d.existing, knownName(d.existing)); return }
        hostKey = d.hostKey || ""
        if (hostKey === "") goTo(3)
        else { status = ""; Qt.callLater(() => trustButton.forceActiveFocus()) }
    }

    Component.onCompleted: {
        if (!backend) return
        searching = true
        pending.discover = backend.discover()
        pending.shares = backend.shares()
        Qt.callLater(focusStep)
    }
    onActiveFocusChanged: if (activeFocus) Qt.callLater(focusStep)

    Connections {
        target: flow.backend
        function onServersDiscovered(requestId, list) {
            if (requestId !== flow.pending.discover) return
            flow.searching = false
            flow.servers = list
            if (flow.step === 1 && !hostField.activeFocus) Qt.callLater(flow.focusStep)
        }
        function onMusicDetailReady(requestId, kind, detail) {
            if (requestId === flow.pending.shares && kind === "shares") flow.known = detail.shares || []
            else if (requestId === flow.pending.signIn && kind === "shareSignIn") flow.signedIn(detail)
        }
        function onRequestFailed(requestId, kind, message) {
            if (requestId === flow.pending.discover) { flow.searching = false; return }
            if (requestId !== flow.pending.signIn) return
            flow.busy = false
            flow.status = message
            // An SMB server that would not list its shares (not a password or network problem):
            // the share is typed instead.
            if (flow.protocol === "smb" && flow.share === "" && kind !== 1 && kind !== 2) {
                flow.typeShare = true
                flow.status = qsTr("The server did not list its shares. Enter the name of the share.")
                Qt.callLater(() => shareField.forceActiveFocus())
            }
            if (flow.protocol === "smb" && !flow.typeShare) flow.share = ""
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 14

        Label { text: qsTr("Add network share"); color: Theme.text; font.pixelSize: Theme.fontTitle; font.bold: true }

        // ---- the stepper, with Back
        RowLayout {
            spacing: 10
            FocusButton {
                id: backButton
                objectName: "back"
                glyph: "←"
                visible: flow.step > 1
                tip: qsTr("Previous step")
                KeyNavigation.down: flow.step === 1 ? (flow.servers.length > 0 ? serverList : protoRow) : flow.step === 2 ? (flow.protocol === "nfs" ? shareField : userField) : folders
                onClicked: flow.back()
            }
            Repeater {
                model: [qsTr("Server"), qsTr("Sign in"), qsTr("Folders")]
                RowLayout {
                    id: stepItem
                    required property string modelData
                    required property int index
                    spacing: 8
                    Rectangle {
                        Layout.preferredWidth: 26; Layout.preferredHeight: 26; radius: 13
                        color: flow.step === stepItem.index + 1 ? Theme.accent : flow.step > stepItem.index + 1 ? Theme.surfaceHi : "transparent"
                        border.width: 1
                        border.color: flow.step >= stepItem.index + 1 ? Theme.accent : Theme.textDim
                        Text {
                            anchors.centerIn: parent
                            text: flow.step > stepItem.index + 1 ? "✓" : String(stepItem.index + 1)
                            color: flow.step === stepItem.index + 1 ? Theme.bg : Theme.text
                            font.pixelSize: Theme.fontSmall
                            font.bold: true
                        }
                    }
                    Label {
                        objectName: "step" + (stepItem.index + 1)
                        text: stepItem.modelData
                        color: flow.step === stepItem.index + 1 ? Theme.text : Theme.textDim
                        font.pixelSize: Theme.fontBody
                        font.bold: flow.step === stepItem.index + 1
                    }
                    Rectangle { visible: stepItem.index < 2; Layout.preferredWidth: 40; Layout.preferredHeight: 1; color: Theme.surfaceHi }
                }
            }
            Item { Layout.fillWidth: true }
            Label {
                visible: flow.step > 1
                text: flow.protocolLabel + "  ·  " + flow.host + (flow.step === 3 && flow.share !== "" ? "  ·  " + flow.share : "")
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
                elide: Text.ElideMiddle
            }
        }

        // ---- step 1: the server
        ColumnLayout {
            visible: flow.step === 1
            Layout.fillWidth: true
            Layout.maximumWidth: 880
            spacing: 12
            RowLayout {
                Label { text: qsTr("Found on your network"); color: Theme.textDim; font.pixelSize: Theme.fontLarge; Layout.fillWidth: true }
                Label {
                    visible: flow.searching
                    text: qsTr("Searching…")
                    color: Theme.textDim
                    font.pixelSize: Theme.fontSmall
                }
            }
            Label {
                visible: flow.servers.length === 0
                text: flow.searching ? qsTr("Looking for file servers…") : qsTr("No servers announced themselves. Enter the address below.")
                color: Theme.textDim
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }
            ListView {
                id: serverList
                objectName: "serverList"
                visible: flow.servers.length > 0
                Layout.fillWidth: true
                Layout.preferredHeight: Math.min(contentHeight, 260)
                clip: true
                spacing: 6
                model: flow.servers
                keyNavigationEnabled: true
                Keys.onUpPressed: (e) => { if (currentIndex <= 0) flow.upFromTop(e); else e.accepted = false }
                Keys.onDownPressed: (e) => { if (currentIndex === count - 1) protoRow.forceActiveFocus(); else e.accepted = false }
                delegate: FocusButton {
                    id: serverButton
                    required property var modelData
                    required property int index
                    objectName: "server" + index
                    width: ListView.view.width
                    focus: ListView.isCurrentItem
                    text: modelData.name + "  ·  " + modelData.protocol.toUpperCase() + (flow.alreadyAdded(modelData) ? "  ·  " + qsTr("already added") : "")
                    tip: modelData.host
                    onClicked: flow.useServer(modelData)
                }
            }

            Label { text: qsTr("Or enter a server"); color: Theme.textDim; font.pixelSize: Theme.fontLarge; Layout.topMargin: 8 }
            Row {
                id: protoRow
                spacing: 8
                onActiveFocusChanged: if (activeFocus) protoRepeater.itemAt(Math.max(0, flow.protocols.findIndex(p => p.key === flow.protocol))).forceActiveFocus()
                Repeater {
                    id: protoRepeater
                    model: flow.protocols
                    delegate: FocusButton {
                        required property var modelData
                        required property int index
                        objectName: "protocol_" + modelData.key
                        text: modelData.label
                        primary: flow.protocol === modelData.key
                        Accessible.role: Accessible.RadioButton
                        Accessible.checked: primary
                        KeyNavigation.left: index > 0 ? protoRepeater.itemAt(index - 1) : null
                        KeyNavigation.right: index < flow.protocols.length - 1 ? protoRepeater.itemAt(index + 1) : null
                        Keys.onUpPressed: (e) => { if (flow.servers.length > 0) serverList.forceActiveFocus(); else flow.upFromTop(e) }
                        KeyNavigation.down: hostField
                        onClicked: flow.protocol = modelData.key
                    }
                }
            }
            RowLayout {
                Layout.fillWidth: true
                spacing: 8
                TextField {
                    id: hostField
                    objectName: "hostField"
                    Layout.fillWidth: true
                    placeholderText: qsTr("Name or address, e.g. nas.local")
                    Accessible.name: qsTr("Server")
                    inputMethodHints: Qt.ImhNoAutoUppercase | Qt.ImhUrlCharactersOnly
                    KeyNavigation.up: protoRow
                    KeyNavigation.right: portField
                    KeyNavigation.down: flow.protocol === "webdav" ? tlsBox : nextButton
                    onAccepted: flow.next()
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
                    KeyNavigation.down: flow.protocol === "webdav" ? tlsBox : nextButton
                    onAccepted: flow.next()
                }
            }
            CheckBox {
                id: tlsBox
                visible: flow.protocol === "webdav"
                text: qsTr("Use HTTPS")
                KeyNavigation.up: hostField
                KeyNavigation.down: nextButton
                contentItem: Label { text: tlsBox.text; color: Theme.text; leftPadding: tlsBox.indicator.width + 8; verticalAlignment: Text.AlignVCenter }
            }
            RowLayout {
                spacing: 12
                FocusButton {
                    id: nextButton
                    objectName: "next"
                    primary: true
                    text: qsTr("Next")
                    KeyNavigation.up: flow.protocol === "webdav" ? tlsBox : hostField
                    onClicked: flow.next()
                }
                Label { text: flow.step === 1 ? flow.status : ""; color: Theme.textDim; Layout.fillWidth: true; wrapMode: Text.Wrap }
            }
        }

        // ---- step 2: signing in
        ColumnLayout {
            visible: flow.step === 2
            Layout.fillWidth: true
            Layout.maximumWidth: 880
            spacing: 12
            GridLayout {
                columns: 2
                columnSpacing: 12
                rowSpacing: 10
                Layout.fillWidth: true

                Label { visible: flow.protocol !== "nfs"; text: qsTr("User name"); color: Theme.text }
                TextField {
                    id: userField
                    objectName: "userField"
                    visible: flow.protocol !== "nfs"
                    Layout.fillWidth: true
                    placeholderText: flow.protocol === "smb" ? qsTr("Empty for guest access") : flow.protocol === "ftp" ? qsTr("Empty for anonymous") : ""
                    Accessible.name: qsTr("User name")
                    inputMethodHints: Qt.ImhNoAutoUppercase | Qt.ImhNoPredictiveText
                    Keys.onUpPressed: (e) => flow.upFromTop(e)
                    KeyNavigation.down: passwordField
                    onTextEdited: flow.smbShares = []
                    onAccepted: passwordField.forceActiveFocus()
                }
                Label {
                    visible: flow.protocol !== "nfs"
                    text: flow.protocol === "sftp" && keyField.text.trim() !== "" ? qsTr("Key passphrase") : qsTr("Password")
                    color: Theme.text
                }
                TextField {
                    id: passwordField
                    objectName: "passwordField"
                    visible: flow.protocol !== "nfs"
                    Layout.fillWidth: true
                    echoMode: TextInput.Password
                    placeholderText: flow.protocol === "sftp" && keyField.text.trim() !== "" ? qsTr("Empty if the key has none") : ""
                    Accessible.name: flow.protocol === "sftp" && keyField.text.trim() !== "" ? qsTr("Key passphrase") : qsTr("Password")
                    inputMethodHints: Qt.ImhSensitiveData | Qt.ImhNoPredictiveText
                    KeyNavigation.up: userField
                    KeyNavigation.down: flow.protocol === "sftp" ? keyField : shareField.visible ? shareField : connectButton
                    onTextEdited: flow.smbShares = []
                    onAccepted: flow.signIn()
                }
                Label { visible: flow.protocol === "sftp"; text: qsTr("Key file (optional)"); color: Theme.text }
                TextField {
                    id: keyField
                    visible: flow.protocol === "sftp"
                    Layout.fillWidth: true
                    placeholderText: qsTr("Log in with a key instead of a password, e.g. ~/.ssh/id_ed25519")
                    Accessible.name: qsTr("Key file")
                    inputMethodHints: Qt.ImhNoAutoUppercase | Qt.ImhNoPredictiveText
                    KeyNavigation.up: passwordField
                    KeyNavigation.down: shareField
                    onAccepted: flow.signIn()
                }
                Label {
                    visible: shareField.visible
                    text: flow.protocol === "smb" ? qsTr("Share") : flow.protocol === "nfs" ? qsTr("Export path") : qsTr("Base folder (optional)")
                    color: Theme.text
                }
                TextField {
                    id: shareField
                    objectName: "shareField"
                    visible: flow.protocol !== "smb" || flow.typeShare
                    Layout.fillWidth: true
                    text: flow.share
                    placeholderText: flow.protocol === "smb" ? qsTr("e.g. media") : flow.protocol === "nfs" ? qsTr("e.g. /volume1/media")
                                   : flow.protocol === "sftp" ? qsTr("e.g. /srv/media") : qsTr("e.g. /dav")
                    Accessible.name: flow.protocol === "smb" ? qsTr("Share") : flow.protocol === "nfs" ? qsTr("Export path") : qsTr("Base folder")
                    inputMethodHints: Qt.ImhNoAutoUppercase
                    Keys.onUpPressed: (e) => { if (flow.protocol === "nfs") flow.upFromTop(e); else (flow.protocol === "sftp" ? keyField : passwordField).forceActiveFocus() }
                    KeyNavigation.down: connectButton
                    onTextEdited: flow.share = text
                    onAccepted: flow.signIn()
                }
            }
            RowLayout {
                spacing: 12
                FocusButton {
                    id: connectButton
                    objectName: "connect"
                    primary: true
                    enabled: !flow.busy
                    text: flow.protocol === "smb" && !flow.typeShare ? qsTr("Show shares") : qsTr("Connect")
                    KeyNavigation.up: shareField.visible ? shareField : passwordField
                    KeyNavigation.down: flow.smbShares.length > 0 ? shareList : (trustButton.visible ? trustButton : null)
                    onClicked: { if (flow.protocol === "smb" && !flow.typeShare) flow.share = ""; flow.signIn() }
                }
                Label {
                    objectName: "signInStatus"
                    text: flow.step === 2 ? flow.status : ""
                    color: Theme.textDim
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    Accessible.role: Accessible.StaticText
                    Accessible.name: text
                }
            }

            // The shares of an SMB server.
            Label {
                visible: flow.smbShares.length > 0
                text: qsTr("Shares on %1").arg(flow.host)
                color: Theme.textDim
                font.pixelSize: Theme.fontLarge
            }
            ListView {
                id: shareList
                objectName: "shareList"
                visible: flow.smbShares.length > 0
                Layout.fillWidth: true
                Layout.preferredHeight: Math.min(contentHeight, 300)
                clip: true
                spacing: 6
                model: flow.smbShares
                keyNavigationEnabled: true
                Keys.onUpPressed: (e) => { if (currentIndex <= 0) connectButton.forceActiveFocus(); else e.accepted = false }
                delegate: FocusButton {
                    required property var modelData
                    required property int index
                    readonly property bool added: modelData.existing !== null && modelData.existing !== undefined
                    objectName: "share" + index
                    width: ListView.view.width
                    focus: ListView.isCurrentItem
                    enabled: !flow.busy
                    text: modelData.name + (added ? "  ·  " + qsTr("already added: add folders") : "")
                    onClicked: flow.chooseShare(modelData)
                }
            }

            // SFTP: the server's key, trusted on first use.
            Label {
                visible: flow.hostKey !== ""
                Layout.fillWidth: true
                wrapMode: Text.WrapAnywhere
                color: Theme.text
                text: qsTr("Server key fingerprint: %1").arg(flow.hostKey)
            }
            Label {
                visible: flow.hostKey !== ""
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.textDim
                text: qsTr("Compare it with the server's own fingerprint before adding (on the server: ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub). Adding the library trusts this key; if the server later presents a different key, the connection is refused.")
            }
            FocusButton {
                id: trustButton
                objectName: "trust"
                visible: flow.hostKey !== ""
                primary: true
                text: qsTr("Trust and continue")
                KeyNavigation.up: connectButton
                onClicked: flow.goTo(3)
            }
            Label {
                Layout.fillWidth: true
                Layout.topMargin: 6
                wrapMode: Text.Wrap
                color: Theme.textDim
                font.pixelSize: Theme.fontSmall
                text: qsTr("The password is kept in your system's keyring once the first folders are added, never in the library.")
            }
        }

        // ---- step 3: the folders
        ShareFolderBrowser {
            id: folders
            visible: flow.step === 3
            Layout.fillWidth: true
            Layout.fillHeight: true
            showTitle: false
            autoBrowse: false
            backend: flow.backend
            target: flow.step === 3 ? flow.spec() : ({})
            password: flow.step === 3 ? flow.password() : ""
            shareName: flow.share
            onAdded: flow.finished()
            onCancelled: flow.goTo(2)
        }
        Item { visible: flow.step < 3; Layout.fillHeight: true }
    }
}
