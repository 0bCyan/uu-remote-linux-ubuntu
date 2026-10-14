import AppKit

// Synthetic acceptance fixture: clipboard values are compared only, never shown or saved.
final class MarkerCheck: NSObject {
    let gnome = NSTextField(labelWithString: "GNOME → Mac: not checked")
    let kde = NSTextField(labelWithString: "KDE → Mac: not checked")
    let copied = NSTextField(labelWithString: "Mac marker: not copied")
    let formats = NSTextField(labelWithString: "Known text formats: not checked")
    // The official viewer drops remote format offers before ACK unless its
    // remote window is key (viewer 0x100380f20 -> isKeyWindow). A delayed
    // marker-only comparison lets it stay key throughout negotiation.
    @objc func armGnome() {
        gnome.stringValue = "GNOME → Mac: armed; return to UU and copy VM marker"
        DispatchQueue.main.asyncAfter(deadline: .now() + 15) { self.compareGnome() }
    }
    @objc func armKde() {
        kde.stringValue = "KDE → Mac: armed; return to UU and copy VM marker"
        DispatchQueue.main.asyncAfter(deadline: .now() + 15) { self.compareKde() }
    }
    @objc func copyMarker() {
        NSPasteboard.general.clearContents()
        let ok = NSPasteboard.general.setString("UU-Mac-to-VM 中文🙂", forType: .string)
        copied.stringValue = ok ? "Mac marker: copied" : "Mac marker: failed"
    }
    func matches() -> Bool {
        let board = NSPasteboard.general
        let utf16 = NSPasteboard.PasteboardType("public.utf16-plain-text")
        let known = board.types ?? []
        formats.stringValue = "Known text formats: UTF-8=\(known.contains(.string)), UTF-16=\(known.contains(utf16))"
        let marker = "UU-VM-to-Mac 中文🚀"
        if board.data(forType: .string) == Data(marker.utf8) { return true }
        guard let actual = board.data(forType: utf16) else { return false }
        for encoding in [String.Encoding.utf16, .utf16LittleEndian, .utf16BigEndian] {
            guard let expected = marker.data(using: encoding) else { continue }
            if actual == expected || actual == expected + Data([0, 0]) { return true }
        }
        return false
    }
    @objc func compareGnome() { gnome.stringValue = matches() ? "GNOME → Mac: PASS" : "GNOME → Mac: not matched" }
    @objc func compareKde() { kde.stringValue = matches() ? "KDE → Mac: PASS" : "KDE → Mac: not matched" }
    @objc func quit() { NSApp.terminate(nil) }
    func button(_ title: String, _ action: Selector) -> NSButton {
        NSButton(title: title, target: self, action: action)
    }
    func show() {
        let window = NSWindow(contentRect: NSRect(x: 0,y: 0,width: 620,height: 500), styleMask: [.titled,.closable], backing: .buffered, defer: false)
        window.title = "UU synthetic clipboard acceptance"
        window.isReleasedWhenClosed = false
        let stack = NSStackView(views: [
            NSTextField(labelWithString: "Only fixed test-marker matches are displayed."),
            NSTextField(labelWithString: "Keep the UU remote window key until format ACK / text request."),
            button("Copy Mac test marker", #selector(copyMarker)), copied,
            button("Compare GNOME VM marker", #selector(compareGnome)), gnome,
            button("Compare KDE VM marker", #selector(compareKde)), kde, formats,
            button("Arm GNOME comparison in 15 seconds", #selector(armGnome)),
            button("Arm KDE comparison in 15 seconds", #selector(armKde)),
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
