// switch.swift
//
// COMPILE:
//   swiftc switch.swift -o switch \
//     -F /System/Library/PrivateFrameworks \
//     -framework SkyLight \
//     -Xlinker -rpath -Xlinker /System/Library/PrivateFrameworks
//
//   ./switch --pid <pid> [--title <title>] [--bounds <x,y,w,h>]

import AppKit
import ApplicationServices
import CoreGraphics

// ─────────────────────────────────────────────────────────────────────────────
// MARK: - Private API declarations
// ─────────────────────────────────────────────────────────────────────────────

@_silgen_name("CGSMainConnectionID")
func CGSMainConnectionID() -> UInt32

@_silgen_name("CGSGetWindowOwner")
@discardableResult
func CGSGetWindowOwner(_ cid: UInt32, _ wid: CGWindowID, _ owner: inout UInt32) -> CGError

@_silgen_name("CGSGetConnectionPSN")
@discardableResult
func CGSGetConnectionPSN(_ cid: UInt32, _ psn: inout ProcessSerialNumber) -> CGError

@_silgen_name("_SLPSSetFrontProcessWithOptions")
@discardableResult
func _SLPSSetFrontProcessWithOptions(_ psn: inout ProcessSerialNumber, _ wid: CGWindowID, _ mode: UInt32) -> CGError

@_silgen_name("SLPSPostEventRecordTo")
@discardableResult
func SLPSPostEventRecordTo(_ psn: inout ProcessSerialNumber, _ bytes: UnsafeMutableRawPointer) -> CGError

@_silgen_name("_AXUIElementGetWindow")
func _AXUIElementGetWindow(_ element: AXUIElement, _ wid: inout CGWindowID) -> AXError

@_silgen_name("GetProcessPID")
func GetProcessPID(_ psn: UnsafePointer<ProcessSerialNumber>, _ pid: inout pid_t) -> OSStatus

@_silgen_name("GetProcessForPID")
func GetProcessForPID(_ pid: pid_t, _ psn: inout ProcessSerialNumber) -> OSStatus

// ─────────────────────────────────────────────────────────────────────────────
// MARK: - SkyLight / WindowServer Nudge
// ─────────────────────────────────────────────────────────────────────────────

func makeKeyWindow(psn: inout ProcessSerialNumber, wid: CGWindowID) {
    var widCopy = wid
    var bytes1 = [UInt8](repeating: 0, count: 0xf8)
    bytes1[0x04] = 0xF8; bytes1[0x08] = 0x01; bytes1[0x3a] = 0x10
    var bytes2 = [UInt8](repeating: 0, count: 0xf8)
    bytes2[0x04] = 0xF8; bytes2[0x08] = 0x02
    withUnsafeMutableBytes(of: &psn) { p in
        bytes1.replaceSubrange(0x18..<0x18+p.count, with: p)
        bytes2.replaceSubrange(0x18..<0x18+p.count, with: p)
    }
    withUnsafeMutableBytes(of: &widCopy) { w in
        bytes1.replaceSubrange(0x3c..<0x3c+w.count, with: w)
        bytes2.replaceSubrange(0x3c..<0x3c+w.count, with: w)
    }
    _ = bytes1.withUnsafeMutableBytes { SLPSPostEventRecordTo(&psn, $0.baseAddress!) }
    _ = bytes2.withUnsafeMutableBytes { SLPSPostEventRecordTo(&psn, $0.baseAddress!) }
}

// ─────────────────────────────────────────────────────────────────────────────
// MARK: - Accessibility Trust Check
// ─────────────────────────────────────────────────────────────────────────────

func checkAccessibilityTrust() {
    let selfPath = Bundle.main.executablePath ?? CommandLine.arguments[0]
    let promptOption = kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String
    let trusted = AXIsProcessTrustedWithOptions([promptOption: true] as CFDictionary)

    if !trusted {
        fputs("Binary path : \(selfPath)\n", stderr)
        fputs("""
        ERROR: This binary is not authorized for Accessibility.
               All AXUIElement calls will fail with kAXErrorAPIDisabled (-25211).
               Fix: System Settings → Privacy & Security → Accessibility
               → add it. Failing that, the binary is missing key permission requests, try a different Terminal.""", stderr)
    } else {
        fputs("----------------------------------\n", stderr)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MARK: - AX helpers
// ─────────────────────────────────────────────────────────────────────────────

extension AXUIElement {
    func axValue<T>(for attribute: String) -> T? {
        var ref: CFTypeRef?
        guard AXUIElementCopyAttributeValue(self, attribute as CFString, &ref) == .success else { return nil }
        return ref as? T
    }
    var cgWindowId: CGWindowID? {
        var wid = CGWindowID(0)
        guard _AXUIElementGetWindow(self, &wid) == .success, wid != 0 else { return nil }
        return wid
    }
}

func wakeElectronAXTree(pid: pid_t) {
    var observer: AXObserver?
    guard AXObserverCreate(pid, { _, _, _, _ in }, &observer) == .success,
          let obs = observer else {
        fputs("  [AXWake] Failed to create observer\n", stderr)
        return
    }
    let app = AXUIElementCreateApplication(pid)
    // Public API - may not be enough for Electron, but try first
    let err = AXObserverAddNotification(obs, app, kAXWindowCreatedNotification as CFString, nil)
    fputs("  [AXWake] AXObserverAddNotification result: \(err.rawValue)\n", stderr)
    CFRunLoopAddSource(CFRunLoopGetCurrent(), AXObserverGetRunLoopSource(obs), .defaultMode)
    // Spin the runloop once so the observer registration actually fires
    CFRunLoopRunInMode(.defaultMode, 0.0, true)
}

// Menu Bar hack: Programmatically click the window title in the app's native Menu Bar
@discardableResult
func axSelectWindowViaMenu(pid: pid_t, targetTitle: String) -> Bool {
    guard !targetTitle.isEmpty else { return false }
    let app = AXUIElementCreateApplication(pid)

    var menuBarRef: CFTypeRef?

    let mbErr = AXUIElementCopyAttributeValue(app, kAXMenuBarAttribute as CFString, &menuBarRef)
    fputs("  [Menu] kAXMenuBarAttribute result: \(mbErr.rawValue)\n", stderr)
    guard mbErr == .success else { return false }
    let menuBar = menuBarRef as! AXUIElement

    var menusRef: CFTypeRef?
    guard AXUIElementCopyAttributeValue(menuBar, kAXChildrenAttribute as CFString, &menusRef) == .success,
          let menus = menusRef as? [AXUIElement] else { return false }

    // Scan top-level menus (File, Edit, View, Window...)
    for menu in menus {
        var menuItemsRef: CFTypeRef?
        guard AXUIElementCopyAttributeValue(menu, kAXChildrenAttribute as CFString, &menuItemsRef) == .success,
              let submenus = menuItemsRef as? [AXUIElement],
              let dropDown = submenus.first else { continue }

        var itemsRef: CFTypeRef?
        guard AXUIElementCopyAttributeValue(dropDown, kAXChildrenAttribute as CFString, &itemsRef) == .success,
              let items = itemsRef as? [AXUIElement] else { continue }

        for item in items {
            var itemTitleRef: CFTypeRef?
            if AXUIElementCopyAttributeValue(item, kAXTitleAttribute as CFString, &itemTitleRef) == .success,
               let itemTitle = itemTitleRef as? String {

                // Account for apps truncating long titles in the menu (e.g. "Braess's parado…")
                var cleanItem = itemTitle
                if cleanItem.hasSuffix("…") { cleanItem.removeLast() }

                if itemTitle == targetTitle || (targetTitle.hasPrefix(cleanItem) && cleanItem.count > 5) {
                    fputs("  [Menu] Found item '\(itemTitle)' — pressing...\n", stderr)
                    let result = AXUIElementPerformAction(item, kAXPressAction as CFString)
                    fputs("  [Menu] AXPerformAction result: \(result.rawValue)\n", stderr)
                    return true
                }
            }
        }
    }
    return false
}

// ─────────────────────────────────────────────────────────────────────────────
// MARK: - The Ultimate Focus Routine
// ─────────────────────────────────────────────────────────────────────────────

func forceFocusWindow(pid: pid_t, targetWid: CGWindowID, targetTitle: String) {
    var axSucceeded = false

    // 1. Accessibility Tree Path (Standard Window tree)
    let axApp = AXUIElementCreateApplication(pid)
    let axWindows: [AXUIElement] = axApp.axValue(for: kAXWindowsAttribute) ?? []

    var match = axWindows.first(where: { $0.cgWindowId == targetWid })
    if match == nil {
        match = axWindows.first(where: {
            let title: String? = $0.axValue(for: kAXTitleAttribute)
            return title == targetTitle && !targetTitle.isEmpty
        })
    }

    if let axWin = match {
        AXUIElementSetAttributeValue(axWin, kAXMainAttribute as CFString, true as CFTypeRef)
        AXUIElementSetAttributeValue(axWin, kAXFocusedAttribute as CFString, true as CFTypeRef)
        AXUIElementPerformAction(axWin, kAXRaiseAction as CFString)
        axSucceeded = true
        fputs("  [Focus] AX raise succeeded.\n", stderr)
    } else {
        fputs("  [Focus] AX raise failed (Window hidden from AX tree).\n", stderr)
    }

    // 2. The Menu Bar Hack (The Ultimate Firefox/Chromium bypass)
    if !axSucceeded && !targetTitle.isEmpty {
        if axSelectWindowViaMenu(pid: pid, targetTitle: targetTitle) {
            fputs("  [Focus] AX Menu Item click succeeded! Native routing triggered.\n", stderr)
            axSucceeded = true
        }
    }

    // 3. SkyLight / CGS Dual-PSN Path (The Full-Screen WindowServer bypass)
    let cid = CGSMainConnectionID()
    var psn = ProcessSerialNumber()
    let psnErr = GetProcessForPID(pid, &psn)

    if psnErr == noErr {
        _SLPSSetFrontProcessWithOptions(&psn, targetWid, 0x2)
        makeKeyWindow(psn: &psn, wid: targetWid)
    }

    var ownerCid = UInt32(0)
    CGSGetWindowOwner(cid, targetWid, &ownerCid)

    var psnFromCGS = ProcessSerialNumber()
    if ownerCid != 0 {
        CGSGetConnectionPSN(ownerCid, &psnFromCGS)
        let psnsDiffer = (psnFromCGS.highLongOfPSN != psn.highLongOfPSN ||
                          psnFromCGS.lowLongOfPSN  != psn.lowLongOfPSN)

        if psnsDiffer {
            fputs("  [Focus] CGS Owner differs from App. Firing full-screen fallback PSN.\n", stderr)
            _SLPSSetFrontProcessWithOptions(&psnFromCGS, targetWid, 0x2)
            makeKeyWindow(psn: &psnFromCGS, wid: targetWid)
        }
    }

    if !axSucceeded {
        fputs("  [Focus] Relied entirely on SkyLight CGS events.\n", stderr)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MARK: - CGWindowList
// ─────────────────────────────────────────────────────────────────────────────

struct WinInfo {
    let wid: CGWindowID
    let pid: pid_t
    let title: String
    let bounds: CGRect
    let onScreen: Bool
}

func allWindows() -> [WinInfo] {
    guard let list = CGWindowListCopyWindowInfo(
        [.optionAll, .excludeDesktopElements], kCGNullWindowID
    ) as? [[String: Any]] else { return [] }
    return list.compactMap { d in
        guard
            let wid  = d[kCGWindowNumber as String] as? CGWindowID,
            let pid  = d[kCGWindowOwnerPID as String] as? pid_t,
            (d[kCGWindowLayer as String] as? Int ?? 999) == 0,
            let bd   = d[kCGWindowBounds as String],
            let bounds = CGRect(dictionaryRepresentation: bd as! CFDictionary),
            bounds.width > 0, bounds.height > 0
        else { return nil }
        return WinInfo(
            wid: wid, pid: pid,
            title: d[kCGWindowName as String] as? String ?? "",
            bounds: bounds,
            onScreen: d[kCGWindowIsOnscreen as String] as? Bool ?? false
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MARK: - Args
// ─────────────────────────────────────────────────────────────────────────────

var targetPID:    pid_t?  = nil
var targetTitle:  String? = nil
var targetBounds: CGRect? = nil

var args = CommandLine.arguments.dropFirst()
while let arg = args.popFirst() {
    switch arg {
    case "--pid":
        if let v = args.popFirst(), let p = pid_t(v) { targetPID = p }
    case "--title":
        targetTitle = args.popFirst()
    case "--bounds":
        if let v = args.popFirst() {
            let p = v.split(separator: ",").compactMap { Double($0) }
            if p.count == 4 { targetBounds = CGRect(x: p[0], y: p[1], width: p[2], height: p[3]) }
        }
    default: break
    }
}

guard let pid = targetPID else {
    fputs("Usage: switch --pid <pid> [--title <title>] [--bounds <x,y,w,h>]\n", stderr)
    exit(1)
}

guard let pid = targetPID else {
    fputs("Usage: switch --pid <pid> [--title <title>] [--bounds <x,y,w,h>]\n", stderr)
    exit(1)
}

checkAccessibilityTrust()

// ─────────────────────────────────────────────────────────────────────────────
// MARK: - Window selection & Diagnostics
// ─────────────────────────────────────────────────────────────────────────────

let allAppWindows = allWindows().filter { $0.pid == pid }
guard !allAppWindows.isEmpty else {
    fputs("ERROR: No layer-0 windows found for PID \(pid). Check Accessibility permission.\n", stderr)
    exit(1)
}

fputs("--- Diagnostic: All layer-0 windows for PID \(pid) ---\n", stderr)
for c in allAppWindows {
    fputs("WID: \(c.wid) | Bounds: \(Int(c.bounds.width))x\(Int(c.bounds.height)) @ \(Int(c.bounds.minX)),\(Int(c.bounds.minY)) | Title: '\(c.title)' | OnScreen: \(c.onScreen)\n", stderr)
}
fputs("------------------------------------------------------\n", stderr)

var candidates = allAppWindows
fputs("Found \(candidates.count) window(s) for PID \(pid).\n", stderr)

if candidates.count > 1, let t = targetTitle {
    let f = candidates.filter { $0.title == t }
    if !f.isEmpty {
        candidates = f
        fputs("After --title filter: \(candidates.count) window(s) remain.\n", stderr)
    } else {
        fputs("Note: --title '\(t)' matched nothing; skipping.\n", stderr)
    }
}
if candidates.count > 1, let b = targetBounds {
    let m: CGFloat = 10
    let f = candidates.filter {
        abs($0.bounds.minX-b.minX)<=m && abs($0.bounds.minY-b.minY)<=m &&
        abs($0.bounds.width-b.width)<=m && abs($0.bounds.height-b.height)<=m
    }
    if !f.isEmpty {
        candidates = f
        fputs("After --bounds filter: \(candidates.count) window(s) remain.\n", stderr)
    }
}
if candidates.count > 1 {
    let on = candidates.filter { $0.onScreen }
    candidates = [on.first ?? candidates[0]]
    fputs("Still ambiguous — picked \(on.isEmpty ? "first" : "on-screen"): '\(candidates[0].title)'\n", stderr)
}

let target = candidates[0]
fputs("Focusing CGWindowID=\(target.wid) '\(target.title)' " +
      "@ \(Int(target.bounds.minX)),\(Int(target.bounds.minY)) " +
      "\(Int(target.bounds.width))×\(Int(target.bounds.height)) " +
      "(onScreen=\(target.onScreen))\n", stderr)

// ─────────────────────────────────────────────────────────────────────────────
// MARK: - Focus Execution
// ─────────────────────────────────────────────────────────────────────────────

guard let runningApp = NSRunningApplication(processIdentifier: pid) else {
    fputs("ERROR: No NSRunningApplication for PID \(pid)\n", stderr)
    exit(1)
}

wakeElectronAXTree(pid: pid)

if target.onScreen {
    // ── Same Space ───────────────────────────────────────────────────────────
    fputs("Window is on current Space — raising directly.\n", stderr)
    forceFocusWindow(pid: pid, targetWid: target.wid, targetTitle: target.title)

} else {
    // ── Other Space ──────────────────────────────────────────────────────────
    fputs("Window is on another Space — switching via NSWorkspace...\n", stderr)

    guard let bundleURL = runningApp.bundleURL else {
        fputs("Warning: app has no bundle URL; falling back to activate()\n", stderr)
        runningApp.activate()
        Thread.sleep(forTimeInterval: 0.4)
        forceFocusWindow(pid: pid, targetWid: target.wid, targetTitle: target.title)
        exit(0)
    }

    let config = NSWorkspace.OpenConfiguration()
    config.activates = true

    let targetWid = target.wid
    let targetTitle = target.title
    let sema = DispatchSemaphore(value: 0)

    NSWorkspace.shared.openApplication(at: bundleURL, configuration: config) { _, error in
        if let error = error {
            fputs("openApplication error: \(error)\n", stderr)
            sema.signal()
            return
        }

        // Wait a beat for the openApplication space switch to settle.
        // Once the app is active, the Menu click will force it to route to the correct internal window.
        Thread.sleep(forTimeInterval: 0.2)
        forceFocusWindow(pid: pid, targetWid: targetWid, targetTitle: targetTitle)

        sema.signal()
    }

    sema.wait()
}