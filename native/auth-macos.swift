import Cocoa
import AuthenticationServices

final class Authentication: NSObject, ASWebAuthenticationPresentationContextProviding {
    let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 420, height: 120),
                          styleMask: [.titled, .closable], backing: .buffered, defer: false)
    var session: ASWebAuthenticationSession?

    func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        window
    }

    func start(_ url: URL) {
        window.title = "Sign in to dev/null with socli"
        window.center()
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        let session = ASWebAuthenticationSession(url: url, callbackURLScheme: "devnull") { callback, error in
            if let callback {
                print(callback.absoluteString)
                fflush(stdout)
                exit(0)
            }
            let cancelled = (error as? ASWebAuthenticationSessionError)?.code == .canceledLogin
            FileHandle.standardError.write(Data((cancelled ? "cancelled" : "The browser could not complete sign-in. Try again, or use manual browser sign-in.").utf8))
            exit(cancelled ? 130 : 1)
        }
        self.session = session
        session.presentationContextProvider = self
        if !session.start() {
            FileHandle.standardError.write(Data("The browser could not start sign-in.".utf8))
            exit(1)
        }
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
guard CommandLine.arguments.count == 2,
      let url = URL(string: CommandLine.arguments[1]),
      url.scheme == "https", url.host == "dev-null.sh", url.path == "/mobile/authorize" else {
    exit(2)
}
let authentication = Authentication()
let parent = getppid()
let lifetime = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { _ in
    if getppid() != parent { exit(130) }
}
DispatchQueue.main.async { authentication.start(url) }
app.run()
