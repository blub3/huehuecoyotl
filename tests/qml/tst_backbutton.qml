import QtQuick
import QtQuick.Controls.Basic
import QtTest
import CoyotlUi

// Issue #133: the Back button over a page stack: hidden on the first page, naming the page
// below (its title, else its kind), shortened at 300 px, going back one page at a time, and the
// page below keeping what it showed.
Item {
    id: root
    width: 1000
    height: 700

    Component {
        id: pageComp
        Flickable {
            property string pageTitle: ""
            property string pageKind: ""
            contentHeight: 3000
        }
    }

    StackView {
        id: stack
        anchors.fill: parent
        pushEnter: null; pushExit: null; popEnter: null; popExit: null
    }
    BackButton {
        id: back
        x: 40; y: 28
        stack: stack
        onBackRequested: stack.pop()
    }
    Icon { id: icon; name: "pause"; size: 26 }

    TestCase {
        name: "BackButton"
        when: windowShown

        function init() {
            stack.clear()
            stack.push(pageComp, { pageTitle: "Artists" })
        }

        function test_hiddenOnFirstPage() {
            verify(!back.visible)
        }

        function test_namesThePageBelowAndGoesBack() {
            stack.push(pageComp, { pageTitle: "Pink Floyd", pageKind: "Artist" })
            stack.get(1).contentY = 800
            stack.push(pageComp, { pageTitle: "The Wall", pageKind: "Album" })
            verify(back.visible)
            compare(back.target, "Pink Floyd")
            compare(back.Accessible.name, "Back to Pink Floyd")
            back.clicked()
            compare(stack.depth, 2)
            compare(back.target, "Artists")
            compare(stack.currentItem.contentY, 800, "the page below is as it was left")
            back.clicked()
            verify(!back.visible)
        }

        function test_kindWhenNoTitle() {
            stack.push(pageComp, { pageKind: "Folder" })
            stack.push(pageComp, { pageTitle: "x" })
            compare(back.target, "Folder")
        }

        function test_longNamesAreShortened() {
            stack.push(pageComp, { pageTitle: "The Lord of the Rings: The Fellowship of the Ring (Extended Edition)" })
            stack.push(pageComp, {})
            verify(back.width <= 300, back.width)
            const label = findChild(back, "backLabel")
            verify(label.truncated)
        }

        function test_keys() {
            stack.push(pageComp, { pageTitle: "Pink Floyd" })
            back.forceActiveFocus()
            keyClick(Qt.Key_Return)
            compare(stack.depth, 1)
            stack.push(pageComp, { pageTitle: "Pink Floyd" })
            back.forceActiveFocus()
            keyClick(Qt.Key_Escape)
            compare(stack.depth, 1)
        }

        function test_iconsKeepTheirSize() {
            compare(icon.width, 26)
            icon.name = "play"
            compare(icon.width, 26)
        }
    }
}
