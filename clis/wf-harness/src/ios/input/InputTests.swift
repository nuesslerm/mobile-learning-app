import XCTest

final class InputTests: XCTestCase {
    let app = XCUIApplication(bundleIdentifier: ProcessInfo.processInfo.environment["WF_IOS_APP_ID"] ?? "host.exp.Exponent")

    private func typeText(_ text: String) {
        // Synchronize each key with the app. A controlled search field can otherwise drop
        // keystrokes while React Native re-renders its results.
        for character in text {
            app.typeText(String(character))
        }
    }

    func testTypeText() throws {
        app.activate()
        let environment = ProcessInfo.processInfo.environment
        guard let session = environment["WF_UI_SESSION"] else {
            typeText(try XCTUnwrap(environment["WF_TEXT"]))
            return
        }

        let documents = try XCTUnwrap(FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first)
        let folder = documents.appendingPathComponent(session)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let request = folder.appendingPathComponent("request.json")
        try Data().write(to: folder.appendingPathComponent("ready"), options: .atomic)
        while true {
            guard FileManager.default.fileExists(atPath: request.path) else {
                Thread.sleep(forTimeInterval: 0.025)
                continue
            }
            let data = try Data(contentsOf: request)
            try FileManager.default.removeItem(at: request)
            let message = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: String])
            let reply = folder.appendingPathComponent(try XCTUnwrap(message["id"]) + ".json")
            let result: [String: Any]
            do {
                switch message["op"] {
                case "stop":
                    try Data("{}".utf8).write(to: reply, options: .atomic)
                    return
                case "tree":
                    var rows = [[String: Any]]()
                    flatten(try app.snapshot(), into: &rows)
                    result = ["value": rows]
                case "tap-button":
                    let identifier = try XCTUnwrap(message["identifier"])
                    let label = try XCTUnwrap(message["label"])
                    let query = app.buttons.matching(NSPredicate(format: "identifier == %@ AND label == %@", identifier, label))
                    let button = query.firstMatch
                    let found = button.exists
                    if found { button.tap() }
                    result = ["value": found]
                case "orientation":
                    let orientations: [String: UIDeviceOrientation] = [
                        "portrait": .portrait, "landscape-left": .landscapeLeft, "landscape-right": .landscapeRight,
                    ]
                    XCUIDevice.shared.orientation = try XCTUnwrap(orientations[message["value"] ?? ""])
                    result = ["value": true]
                case "text":
                    typeText(try XCTUnwrap(message["text"]))
                    result = ["value": true]
                default:
                    result = ["error": "Unknown UI operation"]
                }
            } catch {
                result = ["error": String(describing: error)]
            }
            try JSONSerialization.data(withJSONObject: result).write(to: reply, options: .atomic)
        }
    }

    private func flatten(_ node: XCUIElementSnapshot, into rows: inout [[String: Any]]) {
        let frame = node.frame
        if !frame.isInfinite && !frame.isNull && frame.width > 0 && frame.height > 0 {
            let type: String
            switch node.elementType {
            case .keyboard: type = "Keyboard"
            case .key: type = "Key"
            case .button: type = "Button"
            case .application: type = "Application"
            default: type = "Element"
            }
            rows.append(["AXUniqueId": node.identifier, "AXLabel": node.label,
                         "AXValue": node.value.map { String(describing: $0) } ?? "", "type": type,
                         "frame": ["x": frame.minX, "y": frame.minY, "width": frame.width, "height": frame.height]])
        }
        for child in node.children { flatten(child, into: &rows) }
    }
}
