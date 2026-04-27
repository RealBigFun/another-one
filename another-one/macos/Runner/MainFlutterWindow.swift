import Cocoa
import FlutterMacOS

class MainFlutterWindow: NSWindow {
  private let windowChromeChannelName = "another_one/window_chrome"

  override func awakeFromNib() {
    let flutterViewController = FlutterViewController()
    let windowFrame = self.frame
    self.contentViewController = flutterViewController
    self.setFrame(windowFrame, display: true)

    RegisterGeneratedPlugins(registry: flutterViewController)
    registerWindowChromeChannel(registry: flutterViewController)

    super.awakeFromNib()
  }

  private func registerWindowChromeChannel(registry: FlutterPluginRegistry) {
    let registrar = registry.registrar(forPlugin: "window_chrome")
    let channel = FlutterMethodChannel(
      name: windowChromeChannelName,
      binaryMessenger: registrar.messenger)
    channel.setMethodCallHandler { [weak self] call, result in
      guard let self else {
        result(
          FlutterError(
            code: "NO_WINDOW",
            message: "Window not ready for titlebar actions.",
            details: nil))
        return
      }

      switch call.method {
      case "startWindowDrag":
        guard let event = NSApp.currentEvent else {
          result(
            FlutterError(
              code: "NO_EVENT",
              message: "No active mouse event for window drag.",
              details: nil))
          return
        }
        self.performWindowDrag(with: event)
        result(nil)
      case "toggleMaximize":
        self.zoom(nil)
        result(nil)
      case "minimizeWindow":
        self.miniaturize(nil)
        result(nil)
      case "closeWindow":
        self.performClose(nil)
        result(nil)
      default:
        result(FlutterMethodNotImplemented)
      }
    }
  }
}
