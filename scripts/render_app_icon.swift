#!/usr/bin/env swift

import AppKit
import Foundation
import ImageIO

let repoRoot = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
let defaultInput = URL(fileURLWithPath: "/private/var/folders/l1/vm9308_97b1bdpc1kj98cjdh0000gq/T/codex-clipboard-LBRuzp.png")

let inputURL: URL = {
    if CommandLine.arguments.count > 1 {
        let path = CommandLine.arguments[1]
        if path.hasPrefix("/") {
            return URL(fileURLWithPath: path)
        }
        return repoRoot.appendingPathComponent(path)
    }
    return defaultInput
}()

let sourceOutput = repoRoot.appendingPathComponent("assets/app-icon/source/another-one.png")
let linuxOutput = repoRoot.appendingPathComponent("assets/app-icon/linux/another-one.png")
let macIconset = repoRoot.appendingPathComponent("assets/app-icon/macos/AnotherOne.iconset", isDirectory: true)
let macIcns = repoRoot.appendingPathComponent("assets/app-icon/macos/AnotherOne.icns")

let iconSizes: [(name: String, size: Int)] = [
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1024),
]

enum IconError: Error {
    case couldNotLoadInput(URL)
    case couldNotCreateCGImage
    case couldNotWritePNG(URL)
    case couldNotCreateICNS(URL)
    case couldNotCreateBitmap
}

func makeBitmap(size: Int) -> NSBitmapImageRep {
    NSBitmapImageRep(
        bitmapDataPlanes: nil,
        pixelsWide: size,
        pixelsHigh: size,
        bitsPerSample: 8,
        samplesPerPixel: 4,
        hasAlpha: true,
        isPlanar: false,
        colorSpaceName: .deviceRGB,
        bytesPerRow: 0,
        bitsPerPixel: 0
    )!
}

func savePNG(_ image: NSImage, to url: URL, size: Int) throws -> CGImage {
    let rep = makeBitmap(size: size)
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    NSColor.clear.setFill()
    NSBezierPath(rect: NSRect(x: 0, y: 0, width: size, height: size)).fill()
    NSGraphicsContext.current?.imageInterpolation = .high
    image.draw(in: NSRect(x: 0, y: 0, width: size, height: size))
    NSGraphicsContext.restoreGraphicsState()

    guard let data = rep.representation(using: .png, properties: [:]) else {
        throw IconError.couldNotWritePNG(url)
    }
    try data.write(to: url)

    guard let cgImage = rep.cgImage else {
        throw IconError.couldNotCreateCGImage
    }
    return cgImage
}

func drawLinearGloss(in rect: NSRect) {
    let gloss = NSGradient(colorsAndLocations:
        (NSColor(calibratedWhite: 1.0, alpha: 0.24), 0.0),
        (NSColor(calibratedWhite: 1.0, alpha: 0.05), 0.32),
        (NSColor(calibratedWhite: 1.0, alpha: 0.0), 0.62)
    )
    gloss?.draw(in: NSBezierPath(roundedRect: rect, xRadius: 185, yRadius: 185), angle: 90)
}

func makeTransparentPortrait(from portrait: NSImage) throws -> NSImage {
    let width = Int(portrait.size.width)
    let height = Int(portrait.size.height)
    let bitmap = makeBitmap(size: width)
    bitmap.size = NSSize(width: width, height: height)

    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
    NSColor.clear.setFill()
    NSBezierPath(rect: NSRect(x: 0, y: 0, width: width, height: height)).fill()
    portrait.draw(in: NSRect(x: 0, y: 0, width: width, height: height))
    NSGraphicsContext.restoreGraphicsState()

    guard let data = bitmap.bitmapData else {
        throw IconError.couldNotCreateBitmap
    }

    let bytesPerRow = bitmap.bytesPerRow
    let threshold = UInt8(244)
    var visited = [Bool](repeating: false, count: width * height)
    var queue: [(x: Int, y: Int)] = []
    queue.reserveCapacity(width * 2 + height * 2)

    func offset(_ x: Int, _ y: Int) -> Int {
        y * bytesPerRow + x * 4
    }

    func whiteness(_ x: Int, _ y: Int) -> UInt8 {
        let i = offset(x, y)
        return min(data[i], min(data[i + 1], data[i + 2]))
    }

    func enqueue(_ x: Int, _ y: Int) {
        guard x >= 0, y >= 0, x < width, y < height else { return }
        let index = y * width + x
        guard !visited[index], whiteness(x, y) >= threshold else { return }
        visited[index] = true
        queue.append((x, y))
    }

    for x in 0..<width {
        enqueue(x, 0)
        enqueue(x, height - 1)
    }
    for y in 0..<height {
        enqueue(0, y)
        enqueue(width - 1, y)
    }

    var readIndex = 0
    while readIndex < queue.count {
        let point = queue[readIndex]
        readIndex += 1
        enqueue(point.x - 1, point.y)
        enqueue(point.x + 1, point.y)
        enqueue(point.x, point.y - 1)
        enqueue(point.x, point.y + 1)
    }

    for y in 0..<height {
        for x in 0..<width {
            let index = y * width + x
            guard visited[index] else { continue }
            let i = offset(x, y)
            let white = Int(whiteness(x, y))
            let alpha: UInt8
            if white >= 252 {
                alpha = 0
            } else {
                let scaled = max(0, min(255, (252 - white) * 18))
                alpha = UInt8(scaled)
            }
            data[i + 3] = min(data[i + 3], alpha)
        }
    }

    let transparent = NSImage(size: NSSize(width: width, height: height))
    transparent.addRepresentation(bitmap)
    return transparent
}

func renderBaseIcon(from portrait: NSImage, canvasSize: CGFloat) -> NSImage {
    let image = NSImage(size: NSSize(width: canvasSize, height: canvasSize))
    image.lockFocus()

    let canvas = NSRect(x: 0, y: 0, width: canvasSize, height: canvasSize)
    NSColor.clear.setFill()
    canvas.fill()

    let tileInset = canvasSize * 0.07
    let tileRect = canvas.insetBy(dx: tileInset, dy: tileInset)
    let tilePath = NSBezierPath(roundedRect: tileRect, xRadius: canvasSize * 0.19, yRadius: canvasSize * 0.19)

    NSGraphicsContext.saveGraphicsState()
    let tileShadow = NSShadow()
    tileShadow.shadowColor = NSColor(calibratedWhite: 0.0, alpha: 0.22)
    tileShadow.shadowBlurRadius = canvasSize * 0.032
    tileShadow.shadowOffset = NSSize(width: 0, height: -canvasSize * 0.012)
    tileShadow.set()
    let tileGradient = NSGradient(colorsAndLocations:
        (NSColor(calibratedRed: 0.31, green: 0.33, blue: 0.37, alpha: 1.0), 0.0),
        (NSColor(calibratedRed: 0.18, green: 0.20, blue: 0.23, alpha: 1.0), 0.52),
        (NSColor(calibratedRed: 0.10, green: 0.11, blue: 0.13, alpha: 1.0), 1.0)
    )
    tileGradient?.draw(in: tilePath, angle: 90)
    NSGraphicsContext.restoreGraphicsState()

    NSGraphicsContext.saveGraphicsState()
    tilePath.addClip()

    let radialRect = NSRect(
        x: tileRect.minX - canvasSize * 0.02,
        y: tileRect.midY,
        width: tileRect.width * 0.95,
        height: tileRect.height * 0.72
    )
    let radial = NSGradient(starting: NSColor(calibratedWhite: 1.0, alpha: 0.18), ending: .clear)
    radial?.draw(in: NSBezierPath(ovalIn: radialRect), relativeCenterPosition: NSPoint(x: -0.55, y: 0.42))

    let bottomShade = NSGradient(colorsAndLocations:
        (NSColor.clear, 0.0),
        (NSColor(calibratedWhite: 0.0, alpha: 0.10), 0.68),
        (NSColor(calibratedWhite: 0.0, alpha: 0.24), 1.0)
    )
    bottomShade?.draw(in: tilePath, angle: 90)
    drawLinearGloss(in: tileRect)
    NSGraphicsContext.restoreGraphicsState()

    NSColor(calibratedWhite: 1.0, alpha: 0.22).setStroke()
    tilePath.lineWidth = max(2, canvasSize * 0.003)
    tilePath.stroke()

    let portraitRect = NSRect(
        x: tileRect.minX + canvasSize * 0.06,
        y: tileRect.minY + canvasSize * 0.055,
        width: tileRect.width - canvasSize * 0.10,
        height: tileRect.height - canvasSize * 0.06
    )

    NSGraphicsContext.saveGraphicsState()
    let portraitShadow = NSShadow()
    portraitShadow.shadowColor = NSColor(calibratedWhite: 0.0, alpha: 0.30)
    portraitShadow.shadowBlurRadius = canvasSize * 0.03
    portraitShadow.shadowOffset = NSSize(width: 0, height: -canvasSize * 0.018)
    portraitShadow.set()
    NSGraphicsContext.current?.imageInterpolation = .high
    portrait.draw(in: portraitRect, from: .zero, operation: .sourceOver, fraction: 1.0)
    NSGraphicsContext.restoreGraphicsState()

    image.unlockFocus()
    return image
}

guard let portrait = NSImage(contentsOf: inputURL) else {
    throw IconError.couldNotLoadInput(inputURL)
}
let transparentPortrait = try makeTransparentPortrait(from: portrait)

try FileManager.default.createDirectory(at: sourceOutput.deletingLastPathComponent(), withIntermediateDirectories: true)
try FileManager.default.createDirectory(at: linuxOutput.deletingLastPathComponent(), withIntermediateDirectories: true)
try FileManager.default.createDirectory(at: macIconset, withIntermediateDirectories: true)

let rendered = renderBaseIcon(from: transparentPortrait, canvasSize: 1024)
let sourceCG = try savePNG(rendered, to: sourceOutput, size: 1024)
_ = sourceCG
_ = try savePNG(rendered, to: linuxOutput, size: 256)

var iconsetImages: [CGImage] = []
for icon in iconSizes {
    let cg = try savePNG(rendered, to: macIconset.appendingPathComponent(icon.name), size: icon.size)
    iconsetImages.append(cg)
}

guard let destination = CGImageDestinationCreateWithURL(macIcns as CFURL, "com.apple.icns" as CFString, iconsetImages.count, nil) else {
    throw IconError.couldNotCreateICNS(macIcns)
}

for image in iconsetImages {
    CGImageDestinationAddImage(destination, image, nil)
}

if !CGImageDestinationFinalize(destination) {
    throw IconError.couldNotCreateICNS(macIcns)
}

print("Rendered app icon from \(inputURL.path)")
print("Updated \(sourceOutput.path)")
print("Updated \(linuxOutput.path)")
print("Updated \(macIcns.path)")
