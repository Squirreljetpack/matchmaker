import Foundation
import CoreGraphics
import AppKit

// --- Configuration ---
let filterBlankWindows = true
// ---------------------

let options = CGWindowListOption(arrayLiteral: .optionAll)
guard let windowListInfo = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] else {
    print("Failed to get window list.", to: &standardError)
    exit(1)
}

for window in windowListInfo {
    let layer = window[kCGWindowLayer as String] as? Int ?? 0
    guard layer == 0 else { continue }

    let alpha = window[kCGWindowAlpha as String] as? Double ?? 1.0
    guard alpha > 0.0 else { continue }

    guard let appName = window[kCGWindowOwnerName as String] as? String, !appName.isEmpty else { continue }

    let rawTitle = window[kCGWindowName as String] as? String ?? ""
    if filterBlankWindows && rawTitle.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
        continue
    }
    let title = rawTitle.isEmpty ? "<Blank/No Permission>" : rawTitle

    let rawPid = window[kCGWindowOwnerPID as String] as? Int32 ?? 0
    var pid = rawPid

    // // The Electron Fix: Map background/helper processes to the Main app process
    // if let app = NSRunningApplication(processIdentifier: rawPid) {
    //     // macOS assigns .regular to main apps that appear in the Dock.
    //     // Helper processes get .accessory or .prohibited.
    //     if app.activationPolicy != .regular, let bundleId = app.bundleIdentifier {
    //         let mainApps = NSRunningApplication.runningApplications(withBundleIdentifier: bundleId)
    //         // Find the parent instance that is an actual interactive app
    //         if let mainApp = mainApps.first(where: { $0.activationPolicy == .regular }) {
    //             pid = mainApp.processIdentifier
    //         }
    //     }
    // }

    // Format bounds as "X,Y,W,H" for easy parsing
    var boundsString = "0,0,0,0"
    if let boundsDict = window[kCGWindowBounds as String] as? [String: Any],
       let x = boundsDict["X"] as? CGFloat,
       let y = boundsDict["Y"] as? CGFloat,
       let w = boundsDict["Width"] as? CGFloat,
       let h = boundsDict["Height"] as? CGFloat {
        boundsString = "\(Int(x)),\(Int(y)),\(Int(w)),\(Int(h))"
    }

    // Columns: Title \n App \n PID \n Bounds \0
    print("\(title)\n\(appName)\n\(pid)\n\(boundsString)", terminator: "\0")
}

// Redirect errors to stderr
struct StandardError: TextOutputStream {
    func write(_ string: String) {
        fputs(string, stderr)
    }
}
var standardError = StandardError()