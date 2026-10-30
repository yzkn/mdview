// mdview のモバイル版の、端末の書類（Files）・出力・共有（v3 R-02）。Rust（src/native.rs）から呼ぶ。
//
// - **どの呼び出しも待たない**（画面の糸で選択の画面を出したらすぐ返る）。結果は poll() で受け取る
// - 中身は一時のファイルで受け渡す（Android と同じ形）。開いた書類は一時の所へ写して道を返し、
//   書き戻すときは Rust が書いたファイルを書類へ写す
// - 開いた書類は印（ブックマーク）を持ち、書き戻すたびに「書類へ触る許し」を取り直す
#if os(iOS)
import Foundation
import UIKit
import UniformTypeIdentifiers

@objc(DocumentsPlugin)
public class DocumentsPlugin: NSObject, UIDocumentPickerDelegate {
  private let lock = NSLock()
  private var result: String?
  /// 開いた書類の印（URI の文字 → ブックマーク）
  private var bookmarks: [String: Data] = [:]
  private var exporting = false

  override public init() {
    super.init()
  }

  private func finish(_ kind: String, _ pairs: [String: String] = [:]) {
    var object = pairs
    object["kind"] = kind
    let data = (try? JSONSerialization.data(withJSONObject: object)) ?? Data()
    lock.lock()
    result = String(data: data, encoding: .utf8)
    lock.unlock()
  }

  private func folder(_ name: String) -> URL {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent("mdview/\(name)", isDirectory: true)
    try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
  }

  /// いちばん上に出ている画面
  private func top() -> UIViewController? {
    let window = UIApplication.shared.connectedScenes
      .compactMap { $0 as? UIWindowScene }
      .flatMap { $0.windows }
      .first { $0.isKeyWindow }
    var controller = window?.rootViewController
    while let presented = controller?.presentedViewController {
      controller = presented
    }
    return controller
  }

  @objc public func cacheDir() -> String {
    return folder("work").path
  }

  @objc public func pickOpen() -> String {
    lock.lock(); result = nil; lock.unlock()
    DispatchQueue.main.async {
      let types: [UTType] = [.text, .plainText, UTType(filenameExtension: "md") ?? .text, .data]
      let picker = UIDocumentPickerViewController(forOpeningContentTypes: types, asCopy: false)
      picker.delegate = self
      picker.allowsMultipleSelection = false
      self.exporting = false
      self.top()?.present(picker, animated: true)
    }
    return ""
  }

  public func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
    guard let url = urls.first else {
      finish("cancelled")
      return
    }
    if exporting {
      finish("exported", ["name": url.lastPathComponent])
      return
    }
    let granted = url.startAccessingSecurityScopedResource()
    defer { if granted { url.stopAccessingSecurityScopedResource() } }
    do {
      let data = try Data(contentsOf: url)
      let target = folder("open").appendingPathComponent("document")
      try data.write(to: target)
      if let bookmark = try? url.bookmarkData() {
        bookmarks[url.absoluteString] = bookmark
      }
      finish("opened", ["name": url.lastPathComponent, "uri": url.absoluteString, "path": target.path])
    } catch {
      finish("failed", ["error": error.localizedDescription])
    }
  }

  public func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
    finish("cancelled")
  }

  /// 頼み（JSON）を読む
  private func fields(_ request: String) -> [String: String] {
    guard let data = request.data(using: .utf8),
          let object = try? JSONSerialization.jsonObject(with: data) as? [String: String] else { return [:] }
    return object
  }

  /// `{"uri", "path"}`: `path` の中身を書類 `uri` へ書き戻す。できたら空、できなければ理由
  @objc public func writeBack(_ request: String) -> String {
    let json = fields(request)
    guard let uri = json["uri"], let path = json["path"] else { return "頼みを読めません" }
    var stale = false
    let url: URL
    if let bookmark = bookmarks[uri],
       let resolved = try? URL(resolvingBookmarkData: bookmark, bookmarkDataIsStale: &stale) {
      url = resolved
    } else if let parsed = URL(string: uri) {
      url = parsed
    } else {
      return "書類を指せません"
    }
    let granted = url.startAccessingSecurityScopedResource()
    defer { if granted { url.stopAccessingSecurityScopedResource() } }
    do {
      let data = try Data(contentsOf: URL(fileURLWithPath: path))
      var failure: Error?
      var coordinated: NSError?
      NSFileCoordinator().coordinate(writingItemAt: url, options: .forReplacing, error: &coordinated) { target in
        do { try data.write(to: target, options: .atomic) } catch { failure = error }
      }
      if let error = failure ?? coordinated {
        return error.localizedDescription
      }
      return ""
    } catch {
      return error.localizedDescription
    }
  }

  private func named(_ name: String, _ path: String) -> URL? {
    let target = folder("share").appendingPathComponent(name)
    try? FileManager.default.removeItem(at: target)
    do {
      try FileManager.default.copyItem(at: URL(fileURLWithPath: path), to: target)
      return target
    } catch {
      finish("failed", ["error": error.localizedDescription])
      return nil
    }
  }

  /// `{"name", "mime", "path"}`: 出力の先を選ぶ画面を出す（結果は poll）
  @objc public func pickExport(_ request: String) -> String {
    let json = fields(request)
    guard let name = json["name"], let path = json["path"] else { return "頼みを読めません" }
    lock.lock(); result = nil; lock.unlock()
    DispatchQueue.main.async {
      guard let file = self.named(name, path) else { return }
      let picker = UIDocumentPickerViewController(forExporting: [file], asCopy: true)
      picker.delegate = self
      self.exporting = true
      self.top()?.present(picker, animated: true)
    }
    return ""
  }

  /// `{"name", "mime", "path"}`: 共有の画面を出す
  @objc public func share(_ request: String) -> String {
    let json = fields(request)
    guard let name = json["name"], let path = json["path"] else { return "頼みを読めません" }
    lock.lock(); result = nil; lock.unlock()
    DispatchQueue.main.async {
      guard let file = self.named(name, path), let top = self.top() else { return }
      let sheet = UIActivityViewController(activityItems: [file], applicationActivities: nil)
      // iPad では吹き出しの起点が要る
      sheet.popoverPresentationController?.sourceView = top.view
      sheet.popoverPresentationController?.sourceRect = CGRect(x: top.view.bounds.midX, y: top.view.bounds.maxY - 80, width: 1, height: 1)
      top.present(sheet, animated: true)
      self.finish("shared")
    }
    return ""
  }

  @objc public func openLink(_ href: String) -> String {
    guard let url = URL(string: href) else { return "開けないリンクです" }
    DispatchQueue.main.async {
      UIApplication.shared.open(url)
    }
    return ""
  }

  @objc public func poll() -> String {
    lock.lock()
    defer { lock.unlock() }
    let taken = result ?? ""
    result = nil
    return taken
  }
}
#endif
