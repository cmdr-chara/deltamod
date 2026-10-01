// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import Foundation

// Envelope validation only. Node and Rust still authorize each handoff and
// require user review before any import or managed mutation.
enum LaunchPolicy {
    static let queueLimit = 16
    static let bootstrapPrefix = "GPUIX-LAUNCH-READY "

    static func argumentsAllowed(_ arguments: [String]) -> Bool {
        arguments.count <= 128 && arguments.reduce(0) { $0 + $1.utf8.count + 1 } <= 32768
            && arguments.allSatisfy { !$0.utf8.contains(0) }
    }

    static func file(_ path: String) -> String? {
        guard path.hasPrefix("/"), !path.hasPrefix("//"), path.utf8.count <= 4096,
              !path.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }),
              ["modarchive", "deltamod-open"].contains(URL(fileURLWithPath: path).pathExtension.lowercased())
        else { return nil }
        return path
    }

    static func url(_ url: URL) -> String? {
        if url.isFileURL {
            guard url.host == nil || url.host == "", url.query == nil, url.fragment == nil,
                  url.user == nil, url.password == nil, url.port == nil else { return nil }
            return file(url.path)
        }
        guard let scheme = url.scheme?.lowercased(),
              ["deltamod-gpuix-preview", "deltamod-community"].contains(scheme),
              url.user == nil, url.password == nil, url.port == nil,
              url.fragment == nil, url.absoluteString.utf8.count <= 8192,
              !url.absoluteString.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) })
        else { return nil }
        return url.absoluteString
    }

    static func readiness(_ line: Data, nonce: String) -> [String]? {
        let prefix = Data(bootstrapPrefix.utf8)
        guard line.count <= 32768, line.starts(with: prefix), nonce.utf8.count == 32,
              nonce.utf8.allSatisfy({ (48...57).contains($0) || (97...102).contains($0) }),
              let object = try? JSONSerialization.jsonObject(with: Data(line.dropFirst(prefix.count))),
              let value = object as? [String: String], value.count == 3,
              value["nonce"] == nonce, let state = value["stateRoot"], let managed = value["managedDataRoot"]
        else { return nil }
        let valid: (String) -> Bool = { path in
            path.hasPrefix("/") && path.utf8.count <= 8192
                && !path.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) })
        }
        guard valid(state), managed.isEmpty || valid(managed) else { return nil }
        return ["--state-root", state] + (managed.isEmpty ? [] : ["--managed-data-root", managed])
    }

    private static let failureMessages: [String: (String, String)] = [
        "en": ("Deltamod could not deliver a desktop request", "Delivery was not confirmed. This request will not be retried automatically. Check Deltamod's request list before trying again."),
        "it": ("Deltamod non ha potuto inoltrare una richiesta del desktop", "La consegna non è stata confermata. La richiesta non verrà ripetuta automaticamente. Controlla l'elenco delle richieste di Deltamod prima di riprovare."),
        "de": ("Deltamod konnte eine Desktop-Anfrage nicht weiterleiten", "Die Zustellung wurde nicht bestätigt. Diese Anfrage wird nicht automatisch wiederholt. Prüfe die Anfrageliste in Deltamod, bevor du es erneut versuchst."),
        "es": ("Deltamod no pudo transmitir una solicitud del escritorio", "No se confirmó la entrega. Esta solicitud no se reintentará automáticamente. Revisa la lista de solicitudes de Deltamod antes de volver a intentarlo."),
        "fr": ("Deltamod n'a pas pu transmettre une requête du bureau", "La transmission n'a pas été confirmée. Cette requête ne sera pas réessayée automatiquement. Consultez la liste des requêtes de Deltamod avant de réessayer."),
        "ja": ("Deltamodでデスクトップからの要求を転送できませんでした", "転送を確認できませんでした。この要求は自動的には再試行されません。再試行する前に、Deltamodの要求一覧を確認してください。"),
        "pl": ("Deltamod nie mógł przekazać żądania z pulpitu", "Przekazanie nie zostało potwierdzone. To żądanie nie zostanie automatycznie ponowione. Przed kolejną próbą sprawdź listę żądań w Deltamod."),
        "pt-br": ("O Deltamod não conseguiu encaminhar uma solicitação da área de trabalho", "A entrega não foi confirmada. Esta solicitação não será repetida automaticamente. Confira a lista de solicitações do Deltamod antes de tentar novamente.")
    ]

    static func failureMessage(language: String) -> (String, String) {
        let base = language.lowercased().replacingOccurrences(of: "_", with: "-").split(separator: "-").first.map(String.init) ?? "en"
        return failureMessages[base == "pt" ? "pt-br" : base] ?? failureMessages["en"]!
    }

}
