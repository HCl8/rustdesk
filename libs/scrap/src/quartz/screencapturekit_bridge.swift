import Foundation
import ScreenCaptureKit
import CoreMedia

typealias SCKitFrameCallback = @convention(c) (UnsafePointer<UInt8>, Int32, Int32, Int32) -> Void

class SCKitCaptureManager: NSObject, SCStreamOutput {
    var stream: SCStream?
    var frameCallback: SCKitFrameCallback?
    let callbackQueue = DispatchQueue(label: "com.rustdesk.sckit.capture", qos: .userInteractive)
    var isCapturing = false

    func startCapture(displayID: CGDirectDisplayID, width: Int, height: Int,
                      callback: @escaping SCKitFrameCallback) async throws {
        self.frameCallback = callback

        let content = try await SCShareableContent.excludingDesktopWindows(
            false, onScreenWindowsOnly: false)
        guard let display = content.displays.first(where: { $0.displayID == displayID }) else {
            throw NSError(domain: "SCKit", code: 1,
                          userInfo: [NSLocalizedDescriptionKey: "Display not found"])
        }

        let filter = SCContentFilter(display: display, excludingApplications: [], exceptingWindows: [])
        let config = SCStreamConfiguration()
        config.width = display.width
        config.height = display.height
        config.minimumFrameInterval = CMTime(value: 1, timescale: 30)
        config.pixelFormat = kCVPixelFormatType_32BGRA
        config.showsCursor = true

        let newStream = SCStream(filter: filter, configuration: config, delegate: nil)
        try newStream.addStreamOutput(self, type: .screen, sampleHandlerQueue: callbackQueue)
        try await newStream.startCapture()

        self.stream = newStream
        self.isCapturing = true
    }

    func stopCapture() async throws {
        guard let stream = self.stream else { return }
        try await stream.stopCapture()
        self.stream = nil
        self.isCapturing = false
    }

    // SCStreamOutput delegate
    func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer,
                 of type: SCStreamOutputType) {
        guard type == .screen else { return }
        guard let imageBuffer = sampleBuffer.imageBuffer else { return }
        guard let callback = self.frameCallback else { return }

        CVPixelBufferLockBaseAddress(imageBuffer, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(imageBuffer, .readOnly) }

        guard let baseAddress = CVPixelBufferGetBaseAddress(imageBuffer) else { return }
        let width = Int32(CVPixelBufferGetWidth(imageBuffer))
        let height = Int32(CVPixelBufferGetHeight(imageBuffer))
        let bytesPerRow = Int32(CVPixelBufferGetBytesPerRow(imageBuffer))

        callback(baseAddress.assumingMemoryBound(to: UInt8.self), width, height, bytesPerRow)
    }
}

// --- C-compatible entry points ---

@_cdecl("sckit_is_available")
func sckit_is_available() -> Bool {
    if #available(macOS 12.3, *) {
        return true
    }
    return false
}

@_cdecl("sckit_create")
func sckit_create() -> UnsafeMutableRawPointer? {
    let manager = SCKitCaptureManager()
    return Unmanaged.passRetained(manager).toOpaque()
}

@_cdecl("sckit_release")
func sckit_release(_ ptr: UnsafeMutableRawPointer?) {
    guard let ptr = ptr else { return }
    let manager = Unmanaged<SCKitCaptureManager>.fromOpaque(ptr).takeRetainedValue()
    // Stop capture synchronously if still running
    if manager.isCapturing {
        let semaphore = DispatchSemaphore(value: 0)
        Task {
            try? await manager.stopCapture()
            semaphore.signal()
        }
        semaphore.wait()
    }
}

@_cdecl("sckit_start_capture")
func sckit_start_capture(_ ptr: UnsafeMutableRawPointer?, _ displayID: UInt32,
                          _ width: Int32, _ height: Int32,
                          _ callback: SCKitFrameCallback?) -> Int32 {
    guard let ptr = ptr else { return -1 }
    guard let callback = callback else { return -2 }
    let manager = Unmanaged<SCKitCaptureManager>.fromOpaque(ptr).takeUnretainedValue()

    let semaphore = DispatchSemaphore(value: 0)
    var result: Int32 = 0

    Task {
        do {
            try await manager.startCapture(displayID: CGDirectDisplayID(displayID),
                                            width: Int(width), height: Int(height),
                                            callback: callback)
        } catch {
            result = -3
        }
        semaphore.signal()
    }
    semaphore.wait()
    return result
}

@_cdecl("sckit_stop_capture")
func sckit_stop_capture(_ ptr: UnsafeMutableRawPointer?) -> Int32 {
    guard let ptr = ptr else { return -1 }
    let manager = Unmanaged<SCKitCaptureManager>.fromOpaque(ptr).takeUnretainedValue()

    let semaphore = DispatchSemaphore(value: 0)
    var result: Int32 = 0

    Task {
        do {
            try await manager.stopCapture()
        } catch {
            result = -2
        }
        semaphore.signal()
    }
    semaphore.wait()
    return result
}
