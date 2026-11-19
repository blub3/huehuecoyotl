import QtQuick
import QtTest
import CoyotlUi

// Issue #121: adding a share in three steps with a stub core: a found server, its SMB shares
// (one already added leads to its Add folders), the folders and the share created with them;
// a server typed by hand; a server that does not list its shares; Back and leaving early.
Item {
    id: root
    width: 1200
    height: 800

    QtObject {
        id: stub
        signal serversDiscovered(var requestId, var servers)
        signal musicDetailReady(var requestId, string kind, var detail)
        signal requestFailed(var requestId, int kind, string message)
        property int next: 0
        property var signIns: []
        property var created: null
        property bool refuseList: false
        function later(f) { Qt.callLater(f) }
        function discover() {
            const id = ++next
            later(() => stub.serversDiscovered(id, [{ name: "nas", protocol: "smb", host: "nas.local", port: 445, tls: false }]))
            return id
        }
        function shares() {
            const id = ++next
            later(() => stub.musicDetailReady(id, "shares", { shares: [{ id: 5, name: "backup on nas.local", protocol: "smb", host: "nas.local", folders: [] }], local: [] }))
            return id
        }
        function shareSignIn(spec, password) {
            const id = ++next
            signIns = signIns.concat([{ spec: spec, password: password }])
            if (spec.protocol === "smb" && spec.share === "") {
                if (refuseList) later(() => stub.requestFailed(id, 4, "refused"))
                else later(() => stub.musicDetailReady(id, "shareSignIn", { shares: [{ name: "backup", existing: 5 }, { name: "media", existing: null }] }))
            } else {
                later(() => stub.musicDetailReady(id, "shareSignIn", { connected: true, folders: 1, existing: null, hostKey: "" }))
            }
            return id
        }
        function shareBrowse(target, password, path) {
            const id = ++next
            later(() => stub.musicDetailReady(id, "shareFolders", { share: null, path: "/", parent: "", folders: [
                { name: "Movies", path: "/Movies", guess: "video", looked: true, videos: 3, audio: 0, books: 0, folders: 0, inLibrary: [], inside: null }] }))
            return id
        }
        function shareCreate(spec, password, folders) {
            const id = ++next
            created = { spec: spec, password: password, folders: folders }
            later(() => stub.musicDetailReady(id, "shares", {}))
            return id
        }
        function shareAddFolders(share, folders) { return 0 }
    }

    property var events: []
    Loader {
        id: loader
        anchors.fill: parent
        sourceComponent: AddShareFlow {
            backend: stub
            onOpenExisting: (share, name) => root.events = root.events.concat(["existing " + share + " " + name])
            onFinished: root.events = root.events.concat(["finished"])
            onClosed: root.events = root.events.concat(["closed"])
        }
    }

    TestCase {
        name: "AddShareFlow"
        when: windowShown
        property var flow: null

        function part(name) { return findChild(flow, name) }
        function init() {
            root.events = []
            stub.signIns = []
            stub.created = null
            stub.refuseList = false
            loader.active = false
            loader.active = true
            flow = loader.item
            tryVerify(() => flow.servers.length === 1 && flow.known.length === 1)
        }

        function test_foundServerToNewShare() {
            verify(part("server0").text.indexOf("already added") > 0, "a server with a share in the library says so")
            part("server0").clicked()
            compare(flow.step, 2)
            compare(flow.host, "nas.local")
            part("userField").text = "me"
            part("passwordField").text = "secret"
            part("connect").clicked()
            tryVerify(() => flow.smbShares.length === 2)
            compare(stub.signIns[0].spec.share, "")
            compare(stub.signIns[0].password, "secret")

            tryVerify(() => part("share1") !== null)
            part("share1").clicked()
            tryCompare(flow, "step", 3)
            compare(stub.signIns[1].spec.share, "media")
            tryVerify(() => findChild(flow, "tick0") !== null)
            findChild(flow, "tick0").clicked()
            findChild(flow, "addChosen").clicked()
            tryVerify(() => root.events.indexOf("finished") >= 0)
            compare(stub.created.spec.host, "nas.local")
            compare(stub.created.spec.share, "media")
            compare(stub.created.spec.user, "me")
            compare(stub.created.password, "secret")
            compare(stub.created.folders[0].path, "/Movies")
        }

        function test_existingShareOpensItsFolders() {
            part("server0").clicked()
            part("connect").clicked()
            tryVerify(() => flow.smbShares.length === 2)
            tryVerify(() => part("share0") !== null)
            verify(part("share0").text.indexOf("already added") > 0)
            part("share0").clicked()
            compare(JSON.stringify(root.events), JSON.stringify(["existing 5 backup on nas.local"]))
            compare(stub.created, null)
        }

        function test_typedServerAndBack() {
            part("protocol_nfs").clicked()
            part("hostField").text = "10.0.0.9"
            part("next").clicked()
            compare(flow.step, 2)
            verify(part("shareField").visible, "NFS asks for the export path")
            part("connect").clicked()
            verify(part("signInStatus").text !== "", "an export path is needed")
            part("shareField").text = "/export/media"
            flow.share = "/export/media"
            part("connect").clicked()
            tryCompare(flow, "step", 3)
            compare(stub.signIns[0].spec.protocol, "nfs")
            compare(stub.signIns[0].spec.share, "/export/media")
            // Back twice, then out: nothing was created.
            part("back").clicked()
            compare(flow.step, 2)
            part("back").clicked()
            compare(flow.step, 1)
            part("back").clicked()
            compare(JSON.stringify(root.events), JSON.stringify(["closed"]))
            compare(stub.created, null)
        }

        function test_serverWithoutShareList() {
            stub.refuseList = true
            part("server0").clicked()
            part("connect").clicked()
            tryVerify(() => flow.typeShare)
            verify(part("shareField").visible)
            flow.share = "media"
            part("connect").clicked()
            tryCompare(flow, "step", 3)
            compare(stub.signIns[1].spec.share, "media")
        }
    }
}
