// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import Foundation

@main
struct LaunchPolicyTests {
    static func main() throws {
        var assertions = 0
        func check(_ condition: @autoclosure () -> Bool, _ message: String) {
            assertions += 1
            guard condition() else { fatalError(message) }
        }
        check(LaunchPolicy.file("/tmp/mod.modarchive") != nil, "local archive")
        check(LaunchPolicy.file("/tmp/open.deltamod-open") != nil, "CLI marker")
        for path in ["relative.modarchive", "//server/mod.modarchive", "/tmp/mod.exe", "/tmp/mod\n.modarchive"] {
            check(LaunchPolicy.file(path) == nil, "rejected filename")
        }
        for raw in ["file://server/mod.modarchive", "file:///tmp/mod.modarchive?x=1", "https://example.com/x", "deltamod-gpuix-preview://mod?id=42#fragment"] {
            check(LaunchPolicy.url(URL(string: raw)!) == nil, "rejected URL")
        }
        check(LaunchPolicy.url(URL(string: "deltamod-gpuix-preview://mod?id=42")!) != nil, "preview URL")
        check(!LaunchPolicy.argumentsAllowed(Array(repeating: "x", count: 129)), "argument count")
        check(!LaunchPolicy.argumentsAllowed([String(repeating: "x", count: 32768)]), "argument bytes")
        check(!LaunchPolicy.argumentsAllowed(["nul\0argument"]), "argument NUL")
        let nonce = String(repeating: "a", count: 32)
        func frame(_ values: [String: String]) throws -> Data {
            var data = Data(LaunchPolicy.bootstrapPrefix.utf8)
            data.append(try JSONSerialization.data(withJSONObject: values, options: [.sortedKeys]))
            return data
        }
        let value = ["nonce": nonce, "stateRoot": "/private/state", "managedDataRoot": "/private/managed with spaces"]
        let valid = try frame(value)
        check(LaunchPolicy.readiness(valid, nonce: nonce) == ["--state-root", "/private/state", "--managed-data-root", "/private/managed with spaces"], "identity preserved")
        check(LaunchPolicy.readiness(valid, nonce: String(repeating: "b", count: 32)) == nil, "wrong nonce")
        var invalid = value; invalid["backend"] = "/untrusted/program"
        let extra = try frame(invalid)
        check(LaunchPolicy.readiness(extra, nonce: nonce) == nil, "extra option")
        invalid = value; invalid["stateRoot"] = "--backend"
        let wrongPath = try frame(invalid)
        check(LaunchPolicy.readiness(wrongPath, nonce: nonce) == nil, "relative identity")
        check(LaunchPolicy.readiness(Data(repeating: 120, count: 32769), nonce: nonce) == nil, "frame bytes")
        for language in ["en", "it", "de", "es", "fr", "ja", "pl", "pt-br"] {
            let message = LaunchPolicy.failureMessage(language: language)
            check(!message.0.isEmpty && !message.1.isEmpty, "localized error")
        }
        check(LaunchPolicy.failureMessage(language: "de_DE").0 == LaunchPolicy.failureMessage(language: "de").0, "OS locale normalized")
        check(LaunchPolicy.failureMessage(language: "unsupported").0 == LaunchPolicy.failureMessage(language: "en").0, "English fallback")
        print("LaunchPolicy: \(assertions) focused envelope/handshake/localization assertions passed")
    }
}
