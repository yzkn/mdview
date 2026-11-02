// mdview のモバイル版の、端末の書類（Files）・出力・共有（v3 R-02）。Rust（src/native.rs）から呼ぶ。
//
// - **どの呼び出しも待たない**（画面の糸で選択の画面を出したらすぐ返る）。結果は poll() で受け取る
// - 中身は一時のファイルで受け渡す（Android と同じ形）。開いた書類は一時の所へ写して道を返し、
//   書き戻すときは Rust が書いたファイルを書類へ写す
// - 開いた書類は印（ブックマーク）を持ち、書き戻すたびに「書類へ触る許し」を取り直す
// - 印は UserDefaults にも残し、次に起動しても最近使ったファイルから開ける（v3.1 R-07）
// - 名前を付けて保存は、書き出しを「移す」形（asCopy: false）にして、置いた先の印を取る（v3.1 R-07）
// - 設定・最近使ったファイルの一覧は OS の標準の UserDefaults に持つ（v3.1 R-07・R-08）
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
  /// 名前を付けて保存か（v3.1 R-07）
  private var keeping = false
  /// 印を残す所（UserDefaults の鍵）
  private let bookmarkKey = "mdview.bookmarks"

  override public init() {
    super.init()
    // 前に残した印を読む（最近使ったファイル）
    if let saved = UserDefaults.standard.dictionary(forKey: bookmarkKey) as? [String: Data] {
      bookmarks = saved
    }
  }

  /// 印を覚え、UserDefaults にも残す
  private func remember(_ url: URL) {
    guard let bookmark = try? url.bookmarkData() else { return }
    bookmarks[url.absoluteString] = bookmark
    UserDefaults.standard.set(bookmarks, forKey: bookmarkKey)
  }

  /// 印から書類を指す
  private func resolve(_ uri: String) -> URL? {
    var stale = false
    if let bookmark = bookmarks[uri],
       let resolved = try? URL(resolvingBookmarkData: bookmark, bookmarkDataIsStale: &stale) {
      return resolved
    }
    return URL(string: uri)
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
      self.keeping = false
      self.top()?.present(picker, animated: true)
    }
    return ""
  }

  public func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
    guard let url = urls.first else {
      finish("cancelled")
      return
    }
    if exporting && keeping {
      // 名前を付けて保存: 置いた先の印を取る
      let granted = url.startAccessingSecurityScopedResource()
      defer { if granted { url.stopAccessingSecurityScopedResource() } }
      remember(url)
      finish("saved", ["name": url.lastPathComponent, "uri": url.absoluteString])
      return
    }
    if exporting {
      finish("exported", ["name": url.lastPathComponent])
      return
    }
    copyIn(url)
  }

  /// 書類を一時の所へ写して道を返す（開く・最近使ったファイル）
  private func copyIn(_ url: URL) {
    let granted = url.startAccessingSecurityScopedResource()
    defer { if granted { url.stopAccessingSecurityScopedResource() } }
    do {
      let data = try Data(contentsOf: url)
      let target = folder("open").appendingPathComponent("document")
      try data.write(to: target)
      remember(url)
      finish("opened", ["name": url.lastPathComponent, "uri": url.absoluteString, "path": target.path])
    } catch {
      finish("failed", ["error": error.localizedDescription])
    }
  }

  /// 印（uri）の書類を開く（最近使ったファイル。v3.1 R-07）。結果は poll
  @objc public func openUri(_ uri: String) -> String {
    lock.lock(); result = nil; lock.unlock()
    guard let url = resolve(uri) else { return "書類を指せません" }
    DispatchQueue.global().async {
      self.copyIn(url)
    }
    return ""
  }

  /// 覚えた値（無ければ空。v3.1 R-07・R-08）
  @objc public func prefGet(_ key: String) -> String {
    return UserDefaults.standard.string(forKey: "mdview." + key) ?? ""
  }

  /// `{"key", "value"}`: 値を覚える。できたら空
  @objc public func prefSet(_ request: String) -> String {
    let json = fields(request)
    guard let key = json["key"], let value = json["value"] else { return "頼みを読めません" }
    UserDefaults.standard.set(value, forKey: "mdview." + key)
    return ""
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
    guard let url = resolve(uri) else { return "書類を指せません" }
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

  /// `{"name", "mime", "path", "keep"}`: 出力の先を選ぶ画面を出す（結果は poll）。
  /// `keep` が "true" なら名前を付けて保存（v3.1 R-07）: 移す形で書き出し、置いた先の印を "saved" で返す
  @objc public func pickExport(_ request: String) -> String {
    let json = fields(request)
    guard let name = json["name"], let path = json["path"] else { return "頼みを読めません" }
    let keep = json["keep"] == "true"
    lock.lock(); result = nil; lock.unlock()
    DispatchQueue.main.async {
      guard let file = self.named(name, path) else { return }
      let picker = UIDocumentPickerViewController(forExporting: [file], asCopy: !keep)
      picker.delegate = self
      self.exporting = true
      self.keeping = keep
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
