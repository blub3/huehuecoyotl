import QtQuick
import QtTest
import CoyotlUi

// Issue #171: the notice names the problem, and shows the command only when there is one.
Item {
    width: 600
    height: 300
    CodecNotice { id: notice; width: 500 }
    TestCase {
        name: "CodecNotice"
        when: windowShown
        function test_withCommand() {
            notice.message = "No picture"
            notice.command = "flatpak install flathub x"
            const cmd = findChild(notice, "codecCommand")
            compare(findChild(notice, "codecMessage").text, "No picture")
            compare(cmd.text, "flatpak install flathub x")
            compare(cmd.visible, true)
            compare(cmd.readOnly, true)
        }
        function test_withoutCommand() {
            notice.message = "No sound"
            notice.command = ""
            compare(findChild(notice, "codecCommand").visible, false)
        }
    }
}
