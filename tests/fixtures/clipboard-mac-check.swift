import AppKit

// Synthetic acceptance fixture: clipboard values are compared only, never shown or saved.
final class MarkerCheck: NSObject {
    let gnome = NSTextField(labelWithString: "GNOME → Mac: not checked")
    let kde = NSTextField(labelWithString: "KDE → Mac: not checked")
    let copied = NSTextField(labelWithString: "Mac marker: not copied")
    @objc func copyMarker() {
        NSPasteboard.general.clearContents()
        let ok = NSPasteboard.general.setString("UU-Mac-to-VM 中文🙂", forType: .string)
        copied.stringValue = ok ? "Mac marker: copied" : "Mac marker: failed"
    }
    func matches() -> Bool { NSPasteboard.general.string(forType: .string) == "UU-VM-to-Mac 中文🚀" }
    @objc func compareGnome() { gnome.stringValue = matches() ? "GNOME → Mac: PASS" : "GNOME → Mac: not matched" }
    @objc func compareKde() { kde.stringValue = matches() ? "KDE → Mac: PASS" : "KDE → Mac: not matched" }
    @objc func quit() { NSApp.terminate(nil) }
    func button(_ title: String, _ action: Selector) -> NSButton {
        NSButton(title: title, target: self, action: action)
    }
    func show() {
        let window = NSWindow(contentRect: NSRect(x: 0,y: 0,width: 500,height: 360), styleMask: [.titled,.closable], backing: .buffered, defer: false)
        window.title = "UU synthetic clipboard acceptance"
        window.isReleasedWhenClosed = false
        let stack = NSStackView(views: [
            NSTextField(labelWithString: "Only fixed test-marker matches are displayed."),
            button("Copy Mac test marker", #selector(copyMarker)), copied,
            button("Compare GNOME VM marker", #selector(compareGnome)), gnome,
            button("Compare KDE VM marker", #selector(compareKde)), kde,
            button("Quit test fixture", #selector(quit))])
        stack.orientation = .vertical; stack.spacing = 14
        stack.translatesAutoresizingMaskIntoConstraints = false
        window.contentView!.addSubview(stack)
        NSLayoutConstraint.activate([stack.centerXAnchor.constraint(equalTo: window.contentView!.centerXAnchor),stack.centerYAnchor.constraint(equalTo: window.contentView!.centerYAnchor)])
        window.center(); window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
    }
}
let app = NSApplication.shared
app.setActivationPolicy(.regular)
let fixture = MarkerCheck()
fixture.show()
app.run()
