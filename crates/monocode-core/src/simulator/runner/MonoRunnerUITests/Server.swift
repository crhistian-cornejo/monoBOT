import Foundation
import Network

/// A failure the runner reports to monoBOT as text.
struct RunnerError: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}

/// A minimal HTTP/1.1 server on the loopback interface. Each request is
/// `POST /<action>` with a JSON object body and the session token in
/// `X-Monobot-Token`. Actions run on the main thread, where XCUITest must run.
final class Server {
    private let listener: NWListener
    private let token: String
    private let queue = DispatchQueue(label: "monobot.runner.server")
    private let handle: (String, [String: Any]) throws -> Any
    /// Read and written on the main thread only.
    private(set) var lastRequest = Date()

    init(port: UInt16, token: String, handle: @escaping (String, [String: Any]) throws -> Any) throws {
        guard let port = NWEndpoint.Port(rawValue: port) else { throw RunnerError("Invalid port") }
        let parameters = NWParameters.tcp
        parameters.requiredInterfaceType = .loopback
        parameters.allowLocalEndpointReuse = true
        listener = try NWListener(using: parameters, on: port)
        self.token = token
        self.handle = handle
    }

    func start() {
        listener.newConnectionHandler = { [weak self] connection in
            guard let self else { return }
            connection.start(queue: self.queue)
            self.receive(connection, buffer: Data())
        }
        listener.start(queue: queue)
    }

    func stop() {
        listener.cancel()
    }

    private func receive(_ connection: NWConnection, buffer: Data) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 65536) { [weak self] data, _, done, error in
            guard let self else { return }
            var buffer = buffer
            if let data { buffer.append(data) }
            if error != nil || buffer.count > 1_048_576 {
                connection.cancel()
                return
            }
            if let request = Request(buffer) {
                self.dispatch(request, on: connection)
            } else if done {
                connection.cancel()
            } else {
                self.receive(connection, buffer: buffer)
            }
        }
    }

    private func dispatch(_ request: Request, on connection: NWConnection) {
        guard request.token == token else {
            respond(connection, status: 401, ["ok": false, "error": "Wrong token"])
            return
        }
        let action = String(request.path.drop(while: { $0 == "/" }))
        DispatchQueue.main.async {
            self.lastRequest = Date()
            do {
                let result = try self.handle(action, request.body)
                self.respond(connection, status: 200, ["ok": true, "result": result])
            } catch {
                self.respond(connection, status: 200, ["ok": false, "error": "\(error)"])
            }
        }
    }

    private func respond(_ connection: NWConnection, status: Int, _ object: [String: Any]) {
        let body = (try? JSONSerialization.data(withJSONObject: object)) ?? Data("{}".utf8)
        var data = Data("HTTP/1.1 \(status) \(status == 200 ? "OK" : "Unauthorized")\r\nContent-Type: application/json\r\nContent-Length: \(body.count)\r\nConnection: close\r\n\r\n".utf8)
        data.append(body)
        connection.send(content: data, completion: .contentProcessed { _ in connection.cancel() })
    }
}

/// One complete request, or nil while more bytes are needed.
struct Request {
    let path: String
    let token: String?
    let body: [String: Any]

    init?(_ data: Data) {
        guard let end = data.range(of: Data("\r\n\r\n".utf8)) else { return nil }
        let head = String(decoding: data[data.startIndex..<end.lowerBound], as: UTF8.self)
        var lines = head.components(separatedBy: "\r\n")
        let first = lines.removeFirst().split(separator: " ")
        var length = 0
        var token: String?
        for line in lines {
            let parts = line.split(separator: ":", maxSplits: 1)
            guard parts.count == 2 else { continue }
            let value = parts[1].trimmingCharacters(in: .whitespaces)
            switch parts[0].lowercased() {
            case "content-length": length = Int(value) ?? 0
            case "x-monobot-token": token = value
            default: break
            }
        }
        let start = end.upperBound
        guard data.distance(from: start, to: data.endIndex) >= length else { return nil }
        let body = data[start..<data.index(start, offsetBy: length)]
        self.path = first.count > 1 ? String(first[1]) : "/"
        self.token = token
        self.body = (try? JSONSerialization.jsonObject(with: body)) as? [String: Any] ?? [:]
    }
}
