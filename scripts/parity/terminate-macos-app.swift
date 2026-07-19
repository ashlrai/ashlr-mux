import AppKit
import Darwin
import Foundation

@main
struct TerminateMacOSApp {
    static func main() {
        guard CommandLine.arguments.count == 3,
              let rawPID = Int32(CommandLine.arguments[1]) else {
            fail("usage: terminate-macos-app <pid> <expected-app-bundle>")
        }

        let expectedBundle = URL(fileURLWithPath: CommandLine.arguments[2])
            .standardizedFileURL
            .resolvingSymlinksInPath()
        guard let application = NSRunningApplication(processIdentifier: pid_t(rawPID)) else {
            return
        }
        guard let actualBundle = application.bundleURL?
            .standardizedFileURL
            .resolvingSymlinksInPath(),
              actualBundle == expectedBundle else {
            fail("refusing to terminate pid \(rawPID): bundle does not match \(expectedBundle.path)")
        }

        if !application.terminate(), !application.isTerminated {
            fail("normal termination request was rejected for pid \(rawPID)")
        }

        let deadline = Date().addingTimeInterval(30)
        while !application.isTerminated, Date() < deadline {
            RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        }
        guard application.isTerminated else {
            fail("pid \(rawPID) did not terminate normally within 30 seconds")
        }
    }

    private static func fail(_ message: String) -> Never {
        FileHandle.standardError.write(Data("\(message)\n".utf8))
        Darwin.exit(1)
    }
}
