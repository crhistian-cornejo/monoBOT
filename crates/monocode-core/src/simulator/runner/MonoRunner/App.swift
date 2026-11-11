import SwiftUI

/// The host app XCUITest needs. The runner never shows it: its tests drive
/// whatever app is on the simulator's screen.
@main
struct MonoRunnerApp: App {
    var body: some Scene {
        WindowGroup {
            Text("monoBOT Runner")
        }
    }
}
