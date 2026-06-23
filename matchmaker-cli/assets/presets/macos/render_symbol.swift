import AppKit
import Foundation
import UniformTypeIdentifiers

// Swift helper to render canonical macOS icons
// ONLY resolves UTIs (native system icons). No fallbacks.
// Usage: ./render_symbol <identifier> <output_path>

guard CommandLine.arguments.count >= 3 else {
    print("Usage: render_symbol <identifier> <output_path>")
    exit(1)
}

let identifier = CommandLine.arguments[1]
let outputPath = CommandLine.arguments[2]

let canvasSize = CGSize(width: 256, height: 256)

func saveImage(_ image: NSImage) {
    let targetImage = NSImage(size: canvasSize)
    targetImage.lockFocus()
    NSColor.clear.set()
    NSRect(origin: .zero, size: canvasSize).fill()
    
    // Draw centered and scaled
    let imageSize = image.size
    let ratio = min(canvasSize.width / imageSize.width, canvasSize.height / imageSize.height)
    let newSize = CGSize(width: imageSize.width * ratio, height: imageSize.height * ratio)
    let origin = CGPoint(x: (canvasSize.width - newSize.width) / 2, y: (canvasSize.height - newSize.height) / 2)
    
    image.draw(in: NSRect(origin: origin, size: newSize))
    targetImage.unlockFocus()
    
    if let tiffData = targetImage.tiffRepresentation,
       let bitmap = NSBitmapImageRep(data: tiffData),
       let pngData = bitmap.representation(using: .png, properties: [:]) {
        try? pngData.write(to: URL(fileURLWithPath: outputPath))
    }
}

// Resolve as a UTI
if #available(macOS 12.0, *) {
    if let utType = UTType(identifier), utType.identifier != "public.data" {
        let icon = NSWorkspace.shared.icon(for: utType)
        saveImage(icon)
        exit(0)
    }
}

// Exit without rendering anything if no native UTI was resolved
exit(1)
