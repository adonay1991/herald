// herald-notify — minimal native presenter using the modern notification API
// (UNUserNotificationCenter). The legacy API (NSUserNotification, used by
// osascript and terminal-notifier) is dead on macOS 26: it exits 0 and shows
// nothing. This binary, inside an ad-hoc signed .app bundle, is a sender
// macOS actually registers and authorizes.
// Usage: herald-notify <title> <message> [sound] | herald-notify status

import Foundation
import UserNotifications

func waitUntil(_ done: () -> Bool) {
    while !done() {
        RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(0.05))
    }
}

let args = CommandLine.arguments

if args.count == 2 && args[1] == "status" {
    var done = false
    UNUserNotificationCenter.current().getNotificationSettings { s in
        let state: String
        switch s.authorizationStatus {
        case .notDetermined: state = "notDetermined (never asked)"
        case .denied: state = "denied"
        case .authorized: state = "authorized"
        case .provisional: state = "provisional"
        case .ephemeral: state = "ephemeral"
        @unknown default: state = "unknown(\(s.authorizationStatus.rawValue))"
        }
        print("authorizationStatus: \(state)")
        done = true
    }
    waitUntil { done }
    exit(0)
}

guard args.count >= 3 else {
    FileHandle.standardError.write(Data("usage: herald-notify <title> <message> [sound] | herald-notify status\n".utf8))
    exit(2)
}
let title = args[1]
let body = args[2]
let soundName = args.count > 3 ? args[3] : nil

let center = UNUserNotificationCenter.current()

var authDone = false
var granted = false
var authError: Error?
center.requestAuthorization(options: [.alert, .sound]) { ok, err in
    granted = ok
    authError = err
    authDone = true
}
waitUntil { authDone }

if let err = authError {
    FileHandle.standardError.write(Data("auth error: \(err.localizedDescription)\n".utf8))
    exit(3)
}
if !granted {
    FileHandle.standardError.write(Data("notification permission not granted\n".utf8))
    exit(3)
}

let content = UNMutableNotificationContent()
content.title = title
content.body = body
if let s = soundName {
    content.sound = (s == "default") ? .default : UNNotificationSound(named: UNNotificationSoundName(s))
}

var addDone = false
var addError: Error?
center.add(UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil)) { err in
    addError = err
    addDone = true
}
waitUntil { addDone }

if let err = addError {
    FileHandle.standardError.write(Data("add error: \(err.localizedDescription)\n".utf8))
    exit(4)
}
Thread.sleep(forTimeInterval: 0.3)
exit(0)
