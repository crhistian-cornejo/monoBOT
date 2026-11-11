import XCTest

/// The one test monoBOT runs: it serves actions until told to stop or left
/// idle, then ends. Port, token and idle time come from monoBOT through
/// xcodebuild's TEST_RUNNER_ environment.
final class RunnerUITests: XCTestCase {
    func testServe() throws {
        let environment = ProcessInfo.processInfo.environment
        guard let port = environment["MONOBOT_RUNNER_PORT"].flatMap(UInt16.init),
              let token = environment["MONOBOT_RUNNER_TOKEN"], !token.isEmpty
        else {
            throw RunnerError("MONOBOT_RUNNER_PORT and MONOBOT_RUNNER_TOKEN are required")
        }
        let idle = environment["MONOBOT_RUNNER_IDLE"].flatMap(TimeInterval.init) ?? 1800
        let actions = Actions()
        let server = try Server(port: port, token: token) { action, body in
            try actions.perform(action, body)
        }
        server.start()
        while !actions.stopped && Date().timeIntervalSince(server.lastRequest) < idle {
            RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.1))
        }
        server.stop()
    }
}

private func number(_ value: Any?) -> CGFloat? {
    (value as? NSNumber).map { CGFloat(truncating: $0) }
}

/// What monoBOT can do on the simulator. Coordinates are screen points.
final class Actions {
    var stopped = false
    private let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")

    /// The app a call addresses, else the home screen. Querying an app that
    /// is not in front blocks XCUITest for a long time, so that fails at once,
    /// or falls back to the home screen when monoBOT only guessed the app.
    private func app(_ body: [String: Any]) throws -> XCUIApplication {
        guard let id = body["bundle_id"] as? String, !id.isEmpty,
              id != "com.apple.springboard"
        else { return springboard }
        let app = XCUIApplication(bundleIdentifier: id)
        if app.state == .runningForeground { return app }
        if body["guessed"] as? Bool == true { return springboard }
        throw RunnerError("\(id) is not in front. Launch it first, or leave out bundle_id to use the home screen.")
    }

    /// A point on the screen, whichever app is in front.
    private func point(_ body: [String: Any], _ x: String, _ y: String) throws -> XCUICoordinate {
        guard let px = number(body[x]), let py = number(body[y]) else {
            throw RunnerError("\(x) and \(y) are required, in points")
        }
        return springboard.coordinate(withNormalizedOffset: .zero)
            .withOffset(CGVector(dx: px, dy: py))
    }

    /// The first element whose label or accessibility identifier is `name`.
    private func element(_ body: [String: Any], _ name: String) throws -> XCUIElement {
        let match = try app(body).descendants(matching: .any)
            .matching(NSPredicate(format: "label == %@ OR identifier == %@", name, name))
            .firstMatch
        guard match.waitForExistence(timeout: 3) else {
            throw RunnerError("No element labelled or identified as '\(name)'. Call describe_ui to see what is on screen.")
        }
        return match
    }

    func perform(_ action: String, _ body: [String: Any]) throws -> Any {
        switch action {
        case "status":
            let frame = springboard.frame
            return ["width": frame.width, "height": frame.height]
        case "tap":
            let hold = number(body["duration"]) ?? 0
            if let name = body["element"] as? String, !name.isEmpty {
                let target = try element(body, name)
                hold > 0 ? target.press(forDuration: hold) : target.tap()
                return ["frame": rect(target.frame)]
            }
            let target = try point(body, "x", "y")
            if (body["count"] as? Int) == 2 {
                target.doubleTap()
            } else if hold > 0 {
                target.press(forDuration: hold)
            } else {
                target.tap()
            }
            return [:]
        case "swipe":
            let from = try point(body, "from_x", "from_y")
            let to = try point(body, "to_x", "to_y")
            let velocity: XCUIGestureVelocity
            switch body["speed"] as? String {
            case "slow": velocity = .slow
            case "fast": velocity = .fast
            default: velocity = .default
            }
            from.press(forDuration: 0.05, thenDragTo: to, withVelocity: velocity, thenHoldForDuration: 0)
            return [:]
        case "type_text":
            guard let text = body["text"] as? String, !text.isEmpty else {
                throw RunnerError("text is required")
            }
            try app(body).typeText(text)
            return [:]
        case "press_key":
            let key = try self.key(body["key"] as? String ?? "")
            let count = max(1, min(body["count"] as? Int ?? 1, 100))
            let target = try app(body)
            let flags = modifiers(body["modifiers"] as? [String] ?? [])
            if flags.isEmpty {
                // typeKey needs a hardware keyboard; plain keys go through
                // the on-screen one as text.
                target.typeText(String(repeating: key, count: count))
            } else {
                for _ in 0..<count {
                    target.typeKey(key, modifierFlags: flags)
                }
            }
            return [:]
        case "press_button":
            guard (body["button"] as? String) == "home" else {
                throw RunnerError("button must be 'home'")
            }
            XCUIDevice.shared.press(.home)
            return [:]
        case "rotate":
            let orientations: [String: UIDeviceOrientation] = [
                "portrait": .portrait,
                "portrait_upside_down": .portraitUpsideDown,
                "landscape_left": .landscapeLeft,
                "landscape_right": .landscapeRight,
            ]
            guard let orientation = orientations[body["orientation"] as? String ?? ""] else {
                throw RunnerError("orientation must be portrait, portrait_upside_down, landscape_left or landscape_right")
            }
            XCUIDevice.shared.orientation = orientation
            return [:]
        case "describe":
            let target = try app(body)
            let limit = body["max_lines"] as? Int ?? 600
            return ["text": Tree.render(try target.snapshot(), maxLines: max(20, min(limit, 3000)))]
        case "screen":
            // The live pane's frame: the screen at `scale` × its size in points.
            let scale = max(1, min(number(body["scale"]) ?? 2, 3))
            let quality = max(0.3, min(number(body["quality"]) ?? 0.7, 0.95))
            // The image's size is in points; asking SpringBoard for the
            // screen's frame would cost a third of a second per frame.
            let shot = XCUIScreen.main.screenshot().image
            let points = shot.size
            let format = UIGraphicsImageRendererFormat()
            format.scale = scale
            format.opaque = true
            let frame = UIGraphicsImageRenderer(size: points, format: format).image { _ in
                shot.draw(in: CGRect(origin: .zero, size: points))
            }
            guard let jpeg = frame.jpegData(compressionQuality: quality) else {
                throw RunnerError("The screen could not be encoded")
            }
            return ["jpeg": jpeg.base64EncodedString(), "width": points.width, "height": points.height]
        case "stop":
            stopped = true
            return [:]
        default:
            throw RunnerError("Unknown action '\(action)'")
        }
    }

    private func key(_ name: String) throws -> String {
        switch name.lowercased() {
        case "return", "enter": return XCUIKeyboardKey.return.rawValue
        case "delete", "backspace": return XCUIKeyboardKey.delete.rawValue
        case "forward_delete": return XCUIKeyboardKey.forwardDelete.rawValue
        case "tab": return XCUIKeyboardKey.tab.rawValue
        case "escape": return XCUIKeyboardKey.escape.rawValue
        case "space": return XCUIKeyboardKey.space.rawValue
        case "up": return XCUIKeyboardKey.upArrow.rawValue
        case "down": return XCUIKeyboardKey.downArrow.rawValue
        case "left": return XCUIKeyboardKey.leftArrow.rawValue
        case "right": return XCUIKeyboardKey.rightArrow.rawValue
        case "home": return XCUIKeyboardKey.home.rawValue
        case "end": return XCUIKeyboardKey.end.rawValue
        case let letter where letter.count == 1: return letter
        default:
            throw RunnerError("Unknown key '\(name)'")
        }
    }

    private func modifiers(_ names: [String]) -> XCUIElement.KeyModifierFlags {
        var flags: XCUIElement.KeyModifierFlags = []
        for name in names {
            switch name.lowercased() {
            case "command", "cmd": flags.insert(.command)
            case "shift": flags.insert(.shift)
            case "option", "alt": flags.insert(.option)
            case "control", "ctrl": flags.insert(.control)
            default: break
            }
        }
        return flags
    }
}

func rect(_ frame: CGRect) -> [String: Int] {
    ["x": Int(frame.minX.rounded()), "y": Int(frame.minY.rounded()),
     "width": Int(frame.width.rounded()), "height": Int(frame.height.rounded())]
}
