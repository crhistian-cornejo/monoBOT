import XCTest

/// The accessibility tree as short indented lines an agent can read: kind,
/// label, identifier, value, state, and the point at the element's centre,
/// which `tap` takes as it is. Containers without text only indent.
enum Tree {
    static func render(_ root: XCUIElementSnapshot, maxLines: Int) -> String {
        var lines: [String] = []
        var truncated = false
        let screen = root.frame
        func visit(_ node: XCUIElementSnapshot, depth: Int) {
            // Hidden keyboards and scrolled-away content sit off screen.
            guard node.frame.intersects(screen) else { return }
            if lines.count >= maxLines {
                truncated = true
                return
            }
            var childDepth = depth
            if worthListing(node) {
                lines.append(String(repeating: "  ", count: depth) + describe(node))
                childDepth += 1
            }
            for child in node.children {
                visit(child, depth: childDepth)
            }
        }
        visit(root, depth: 0)
        if truncated {
            lines.append("… more elements not listed; pass a larger max_lines")
        }
        return lines.joined(separator: "\n")
    }

    private static func text(_ node: XCUIElementSnapshot) -> String? {
        guard node.elementType != .secureTextField, let value = node.value else { return nil }
        let text = "\(value)"
        return text.isEmpty || text == node.label ? nil : text
    }

    private static func worthListing(_ node: XCUIElementSnapshot) -> Bool {
        guard node.frame.width > 0, node.frame.height > 0 else { return false }
        if !node.label.isEmpty || text(node) != nil { return true }
        // Without text, only what can be used is worth a line.
        switch node.elementType {
        case .button, .textField, .secureTextField, .searchField, .textView, .switch, .toggle,
             .slider, .stepper, .picker, .pickerWheel, .datePicker, .segmentedControl, .link,
             .cell, .tab, .menuItem, .key:
            return true
        default:
            return false
        }
    }

    private static func clip(_ text: String) -> String {
        let flat = text.replacingOccurrences(of: "\n", with: " ")
        return flat.count > 120 ? String(flat.prefix(120)) + "…" : flat
    }

    private static func describe(_ node: XCUIElementSnapshot) -> String {
        var parts = [name(node.elementType)]
        if !node.label.isEmpty { parts.append("\"\(clip(node.label))\"") }
        if !node.identifier.isEmpty { parts.append("id=\(clip(node.identifier))") }
        if let value = text(node) { parts.append("value=\"\(clip(value))\"") }
        if let placeholder = node.placeholderValue, !placeholder.isEmpty {
            parts.append("placeholder=\"\(clip(placeholder))\"")
        }
        if node.elementType == .secureTextField { parts.append("secure") }
        if !node.isEnabled { parts.append("disabled") }
        if node.isSelected { parts.append("selected") }
        if node.hasFocus { parts.append("focused") }
        let frame = node.frame
        parts.append("at (\(Int(frame.midX.rounded())), \(Int(frame.midY.rounded()))) size \(Int(frame.width.rounded()))×\(Int(frame.height.rounded()))")
        return parts.joined(separator: " ")
    }

    private static func name(_ type: XCUIElement.ElementType) -> String {
        switch type {
        case .alert: return "Alert"
        case .application: return "App"
        case .button: return "Button"
        case .cell: return "Cell"
        case .checkBox: return "CheckBox"
        case .collectionView: return "Collection"
        case .datePicker: return "DatePicker"
        case .icon: return "Icon"
        case .image: return "Image"
        case .key: return "Key"
        case .keyboard: return "Keyboard"
        case .link: return "Link"
        case .map: return "Map"
        case .menu: return "Menu"
        case .menuItem: return "MenuItem"
        case .navigationBar: return "NavigationBar"
        case .other: return "Other"
        case .pageIndicator: return "PageIndicator"
        case .picker: return "Picker"
        case .pickerWheel: return "PickerWheel"
        case .progressIndicator: return "Progress"
        case .activityIndicator: return "Activity"
        case .scrollView: return "ScrollView"
        case .searchField: return "SearchField"
        case .secureTextField: return "SecureTextField"
        case .segmentedControl: return "SegmentedControl"
        case .sheet: return "Sheet"
        case .slider: return "Slider"
        case .staticText: return "Text"
        case .statusBar: return "StatusBar"
        case .stepper: return "Stepper"
        case .switch: return "Switch"
        case .tab: return "Tab"
        case .tabBar: return "TabBar"
        case .table: return "Table"
        case .textField: return "TextField"
        case .textView: return "TextView"
        case .toggle: return "Toggle"
        case .toolbar: return "Toolbar"
        case .webView: return "WebView"
        case .window: return "Window"
        default: return "Element"
        }
    }
}
