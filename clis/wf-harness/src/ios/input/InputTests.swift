import XCTest

final class InputTests: XCTestCase {
    func testTypeText() throws {
        let app = XCUIApplication(bundleIdentifier: "host.exp.Exponent")
        app.activate()
        let text = try XCTUnwrap(ProcessInfo.processInfo.environment["WF_TEXT"])
        app.typeText(text)
    }
}
