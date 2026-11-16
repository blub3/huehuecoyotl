import QtQuick
import QtTest
import CoyotlUi

// Issue #52: what Settings shows for each state of the TMDB key, and that the key itself is never
// part of it.
Item {
    id: root
    width: 900
    height: 400
    property var calls: []

    TmdbKeyStatus {
        id: status
        width: 860
        onReplaceRequested: root.calls = root.calls.concat(["replace"])
        onTestRequested: root.calls = root.calls.concat(["test"])
        onRemoveRequested: root.calls = root.calls.concat(["remove"])
    }

    TestCase {
        name: "TmdbKeyStatus"
        when: windowShown

        function part(name) { return findChild(status, name) }
        function init() {
            root.calls = []
            status.validatedAt = null
            status.busy = false
        }

        function test_storedKey() {
            status.keyState = "keystore"
            status.validatedAt = new Date(2026, 10, 16, 9, 30)
            verify(status.visible)
            compare(part("keyHeadline").text, "Key stored")
            verify(part("keyDetail").text.indexOf("system keystore") >= 0)
            verify(part("keyChecked").text.indexOf("Last accepted by TMDB") === 0)
            for (const b of ["replaceKey", "testKey", "removeKey"]) verify(part(b).visible, b)
            status.firstButton.forceActiveFocus()
            verify(part("replaceKey").activeFocus)
            keyClick(Qt.Key_Right)
            verify(part("testKey").activeFocus)
            keyClick(Qt.Key_Return)
            keyClick(Qt.Key_Right)
            keyClick(Qt.Key_Return)
            compare(root.calls, ["test", "remove"])
        }

        function test_neverValidated() {
            status.keyState = "keystore"
            compare(part("keyChecked").text, "Not checked with TMDB yet.")
        }

        function test_keyFromTheEnvironment() {
            status.keyState = "environment"
            verify(part("keyDetail").text.indexOf("COYOTL_TMDB_KEY") >= 0)
            verify(!part("replaceKey").visible, "the environment's key is not replaced here")
            verify(!part("removeKey").visible, "nor removed")
            verify(part("testKey").visible)
            compare(status.firstButton, part("testKey"))
        }

        function test_noKey() {
            status.keyState = "none"
            verify(!status.visible, "the page shows the field and its explanation instead")
            compare(status.firstButton, null)
        }

        function test_keystoreUnavailable() {
            status.keyState = "unavailable"
            verify(status.visible, "an explanation instead of an empty field")
            compare(part("keyHeadline").text, "No system keystore available")
            verify(part("keyDetail").text.indexOf("keyring") >= 0)
            verify(!part("testKey").visible)
            compare(part("keyChecked").text, "")
        }

        function test_keptForThisSessionOnly() {
            status.keyState = "session"
            verify(part("keyDetail").text.indexOf("entered again after a restart") >= 0)
            verify(part("testKey").visible)
        }

        function test_stillLooking() {
            status.keyState = ""
            compare(part("keyHeadline").text, "Looking for the stored key…")
            verify(!part("testKey").visible)
        }

        function test_testingShowsProgress() {
            status.keyState = "keystore"
            status.busy = true
            compare(part("testKey").text, "Checking…")
            part("testKey").forceActiveFocus()
            keyClick(Qt.Key_Return)
            compare(root.calls, [], "a second test waits for the first")
            verify(part("testKey").activeFocus, "focus stays on the button")
        }
    }
}
