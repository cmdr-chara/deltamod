// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import AppKit
import Foundation
import Darwin

/// Launch Services owns this small, windowless native application. GPUIX owns
/// its renderer child. No WebView, delegate swizzling, arbitrary shell command
/// or polling of the managed data directory is involved.
@MainActor
final class Launcher: NSObject, NSApplicationDelegate {
    private var primary: Process?
    private var forwarder: Process?
    private var pending: [[String]] = []
    private var ready = false
    private var forwardContext: [String] = []
    private var failed = false
    private var primaryEnded = false
    private var outputEnded = false
    private var quitting = false
    private var errorShown = false
    private var bootstrapDeadline: DispatchWorkItem?
    private var forwardDeadline: DispatchWorkItem?
    private let nonce = UUID().uuidString.replacingOccurrences(of: "-", with: "").lowercased()

    private func executable() throws -> URL {
        let bundle = Bundle.main.bundleURL.resolvingSymlinksInPath()
        guard bundle.pathExtension == "app" else { throw CocoaError(.fileReadUnsupportedScheme) }
        let file = bundle.appendingPathComponent("Contents/MacOS/deltamod-gpuix")
        let attributes = try FileManager.default.attributesOfItem(atPath: file.path)
        guard attributes[.type] as? FileAttributeType == .typeRegular,
              file.resolvingSymlinksInPath() == file,
              FileManager.default.isExecutableFile(atPath: file.path)
        else { throw CocoaError(.fileReadNoPermission) }
        return file
    }

    private func reportFailure() {
        // Never print provider URLs, user filenames, nonces or credentials.
        FileHandle.standardError.write(Data("Deltamod: desktop delivery failed. No request will be retried automatically.\n".utf8))
        guard !errorShown else { return }
        errorShown = true
        let alert = NSAlert()
        let message = LaunchPolicy.failureMessage(language: Locale.preferredLanguages.first ?? "en")
        alert.messageText = message.0
        alert.informativeText = message.1
        alert.alertStyle = .warning
        alert.runModal()
    }

    private func enqueue(_ arguments: [[String]]) -> Bool {
        guard !failed, !quitting, pending.count + arguments.count + (forwarder == nil ? 0 : 1) <= LaunchPolicy.queueLimit,
              arguments.allSatisfy(LaunchPolicy.argumentsAllowed) else { reportFailure(); return false }
        pending.append(contentsOf: arguments)
        forwardNext()
        return true
    }

    func applicationWillFinishLaunching(_ notification: Notification) {
        // Delegate installation precedes cold open-file/open-URL AppleEvents.
        NSApplication.shared.setActivationPolicy(.accessory)
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        do {
            let arguments = Array(CommandLine.arguments.dropFirst()).filter {
                $0.range(of: #"^-psn_[0-9]+_[0-9]+$"#, options: .regularExpression) == nil
            }
            guard LaunchPolicy.argumentsAllowed(arguments) else { throw CocoaError(.fileReadCorruptFile) }
            let child = Process()
            child.executableURL = try executable()
            child.arguments = arguments
            var environment = ProcessInfo.processInfo.environment
            environment["DELTAMOD_GPUIX_LAUNCH_NONCE"] = nonce
            child.environment = environment
            child.standardInput = FileHandle.nullDevice
            child.standardError = FileHandle.nullDevice
            let output = Pipe()
            child.standardOutput = output
            child.terminationHandler = { [weak self] process in
                DispatchQueue.main.async {
                    guard let self else { return }
                    self.primaryEnded = true
                    if process.terminationStatus != 0 { self.failed = true; self.pending.removeAll(); self.reportFailure() }
                    self.finishIfClosed()
                }
            }
            primary = child
            try child.run()
            readOutput(output.fileHandleForReading)
            let deadline = DispatchWorkItem { [weak self] in
                guard let self, !self.ready else { return }
                self.failed = true
                self.pending.removeAll()
                self.reportFailure()
                // Do not kill a renderer that might be performing recovery.
            }
            bootstrapDeadline = deadline
            DispatchQueue.main.asyncAfter(deadline: .now() + 45, execute: deadline)
        } catch {
            failed = true; pending.removeAll(); primary = nil
            reportFailure(); NSApplication.shared.terminate(nil)
        }
    }

    private func readOutput(_ handle: FileHandle) {
        let expected = nonce
        DispatchQueue.global(qos: .utility).async { [weak self] in
            var buffer = Data()
            var accepted = false
            do {
                while let chunk = try handle.read(upToCount: 4096), !chunk.isEmpty {
                    if accepted { continue } // Drain subsequent output without retaining it.
                    buffer.append(chunk)
                    while let newline = buffer.firstIndex(of: 10) {
                        let line = Data(buffer[..<newline])
                        buffer.removeSubrange(...newline)
                        if let context = LaunchPolicy.readiness(line, nonce: expected) {
                            accepted = true
                            DispatchQueue.main.async { self?.becameReady(context) }
                        }
                    }
                    if buffer.count > 32768 { buffer.removeFirst(buffer.count - 32768) }
                }
            } catch { /* EOF/read failure is handled together with process exit. */ }
            try? handle.close()
            // This is queued after any readiness callback from the same reader.
            DispatchQueue.main.async {
                guard let self else { return }
                self.outputEnded = true
                self.finishIfClosed()
            }
        }
    }

    private func becameReady(_ context: [String]) {
        guard !ready, !failed, !quitting else { return }
        forwardContext = context
        ready = true
        bootstrapDeadline?.cancel(); bootstrapDeadline = nil
        forwardNext()
    }

    func application(_ sender: NSApplication, open urls: [URL]) {
        guard !urls.isEmpty, urls.count <= LaunchPolicy.queueLimit else { reportFailure(); return }
        let requests = urls.compactMap(LaunchPolicy.url)
        guard requests.count == urls.count else { reportFailure(); return }
        _ = enqueue(requests.map { [$0] })
    }

    func application(_ sender: NSApplication, openFiles filenames: [String]) {
        guard !filenames.isEmpty, filenames.count <= LaunchPolicy.queueLimit else {
            reportFailure(); sender.reply(toOpenOrPrint: .failure); return
        }
        let requests = filenames.compactMap(LaunchPolicy.file)
        let accepted = requests.count == filenames.count && enqueue(requests.map { [$0] })
        // Accepted for review, not an assertion that an archive was installed.
        sender.reply(toOpenOrPrint: accepted ? .success : .failure)
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        _ = enqueue([[]]) // An authenticated empty handoff requests foregrounding.
        return false
    }

    func applicationShouldOpenUntitledFile(_ sender: NSApplication) -> Bool { false }

    private func forwardNext() {
        guard ready, !failed, !quitting, forwarder == nil, !pending.isEmpty else { return }
        let arguments = pending.removeFirst()
        do {
            let child = Process()
            child.executableURL = try executable()
            // Even if the real primary dies now, this child cannot elect itself
            // or start a writable Rust backend. No retry after a lost receipt.
            child.arguments = ["--forward-only"] + forwardContext + ["--"] + arguments
            var environment = ProcessInfo.processInfo.environment
            environment.removeValue(forKey: "DELTAMOD_GPUIX_LAUNCH_NONCE")
            child.environment = environment
            child.standardInput = FileHandle.nullDevice
            child.standardOutput = FileHandle.nullDevice
            child.standardError = FileHandle.nullDevice
            child.terminationHandler = { [weak self] process in
                DispatchQueue.main.async {
                    guard let self else { return }
                    self.forwardDeadline?.cancel(); self.forwardDeadline = nil
                    self.forwarder = nil
                    if process.terminationStatus != 0 { self.reportFailure() }
                    self.forwardNext()
                    self.finishIfClosed()
                }
            }
            forwarder = child
            try child.run()
            let deadline = DispatchWorkItem { [weak child] in
                // This process is forward-only, never a writer. Bound hung OS
                // handoffs without ever killing the primary renderer/recovery.
                guard let child, child.isRunning else { return }
                Darwin.kill(child.processIdentifier, SIGKILL)
            }
            forwardDeadline = deadline
            DispatchQueue.main.asyncAfter(deadline: .now() + 8, execute: deadline)
        } catch {
            forwarder = nil; reportFailure()
            // Continue distinct queued requests, never retry the failed one.
            forwardNext(); finishIfClosed()
        }
    }

    private func finishIfClosed() {
        guard primaryEnded, outputEnded else { return }
        if !ready { failed = true; pending.removeAll(); reportFailure() }
        guard forwarder == nil, pending.isEmpty else { return }
        bootstrapDeadline?.cancel()
        if quitting { NSApplication.shared.reply(toApplicationShouldTerminate: true) }
        else { NSApplication.shared.terminate(nil) }
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        quitting = true; pending.removeAll(); bootstrapDeadline?.cancel()
        if let child = forwarder, child.isRunning { child.terminate() }
        if let child = primary, child.isRunning {
            child.terminate() // Explicit OS quit uses the frontend's owned shutdown.
            return .terminateLater
        }
        return .terminateNow
    }
}

@main
struct DesktopLauncher {
    @MainActor static func main() {
        let application = NSApplication.shared
        let delegate = Launcher()
        application.delegate = delegate
        withExtendedLifetime(delegate) { application.run() }
    }
}
