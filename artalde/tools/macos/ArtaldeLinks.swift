import AppKit

// Launch Services delivers every URL in an open event, including a batch from Finder.
final class LinkReceiver: NSObject, NSApplicationDelegate {
    func application(_ application: NSApplication, open urls: [URL]) {
        guard let executable = Bundle.main.object(forInfoDictionaryKey: "ArtaldeExecutable") as? String else { return }
        for url in urls {
            let process = Process()
            process.executableURL = URL(fileURLWithPath: executable)
            process.arguments = ["open", url.absoluteString]
            if let fallback = Bundle.main.object(forInfoDictionaryKey: "ArtaldeFallbackBrowser") as? String {
                process.arguments?.append(contentsOf: ["--fallback-browser", fallback])
            }
            var env = ProcessInfo.processInfo.environment
            let paths = Bundle.main.object(forInfoDictionaryKey: "ArtaldeToolPaths") as? [String] ?? []
            env["PATH"] = (paths + [env["PATH"] ?? "/usr/bin:/bin"]).joined(separator: ":")
            process.environment = env
            do { try process.run() }
            catch { NSLog("Artalde could not open the link: %@", error.localizedDescription) }
        }
    }
}

let receiver = LinkReceiver()
let app = NSApplication.shared
app.delegate = receiver
app.run()
