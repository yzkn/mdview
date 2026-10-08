// mdview の起動用アプリ（v2.1.0 R-08）。**macOS でしか組めない。**
//
// # なぜ要るのか
//
// Finder でファイルをダブルクリックすると、macOS はファイルを**起動引数ではなく
// Apple Event（odoc）で**アプリへ渡す。mdview 本体が使う iced（winit 0.30）は
// これを受け取る口を持たないため、Finder から開くと**ファイルを開かずに起動する**。
//
// そこで .app の本体をこの小さなアプリにし、受け取ったファイルを
// **mdview 本体へ起動引数として渡す**。本体は「窓ごとに別のプロセス」で
// 動くので（R-09）、ファイルの数だけ本体を起こせばよい。
//
// # 作り
//
//   mdview.app/Contents/MacOS/mdview                   ← これ（起動用）
//   mdview.app/Contents/Helpers/mdview.app/…/mdview    ← 本体（Rust）
//
// **本体は別のバンドル ID を持つ入れ子の .app にする。** 同じ ID だと、
// 次に Finder で開いたときに macOS が「もう起動している」と見なし、
// 受け取る口の無い本体へ Apple Event を送ってしまう。
//
// 起動用は渡し終えたら終わる。次に開くときは、また起動用が起こされる。

import Cocoa

/// 本体の実行ファイル。
let editor = Bundle.main.bundleURL
    .appendingPathComponent("Contents/Helpers/mdview.app/Contents/MacOS/mdview")

// --- 端末から起こされたとき ---
//
// `mdview.app/Contents/MacOS/mdview --version` のように引数付きで起こされたら、
// **本体に置き換わる**（execv）。標準出力も終了コードも本体のものになるので、
// 端末からの使い方と CI の煙試験がそのまま通る。
// Finder が古い macOS で付ける `-psn_…` は引数として扱わない
let forwarded = CommandLine.arguments.dropFirst().filter { !$0.hasPrefix("-psn_") }
if !forwarded.isEmpty {
    let path = editor.path
    var argv: [UnsafeMutablePointer<CChar>?] = ([path] + forwarded).map { strdup($0) }
    argv.append(nil)
    execv(path, &argv)
    // ここへ来たのは置き換われなかったとき
    perror("mdview: 本体を起動できません")
    exit(127)
}

final class Launcher: NSObject, NSApplicationDelegate {
    /// Finder からファイルを受け取ったか
    private var received = false
    /// 終わる予約（受け取るたびに延ばす）
    private var quit: DispatchWorkItem?

    /// Finder から開かれた（起動前・起動後のどちらでも届く）。
    func application(_ application: NSApplication, open urls: [URL]) {
        let files = urls.filter { $0.isFileURL }.map { $0.path }
        guard !files.isEmpty else { return }
        received = true
        start(files)
        scheduleQuit()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        // **ファイル付きで開かれたときは、ここより先に `open` が届く。**
        // 少しだけ待ち、何も届かなければ空の窓を開く（アプリそのものを開いた）
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
            if !self.received {
                self.start([])
            }
            self.scheduleQuit()
        }
    }

    /// Dock などからもう一度開かれた（起動用が生きている間だけ）
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        start([])
        scheduleQuit()
        return false
    }

    /// 本体を起こす。**ファイルが複数なら、本体が 2 つ目から別の窓にする**（R-09）
    private func start(_ files: [String]) {
        let process = Process()
        process.executableURL = editor
        process.arguments = files
        do {
            try process.run()
        } catch {
            // **黙って終わらない。** 何も起きないのが一番困る
            let alert = NSAlert()
            alert.messageText = "mdview を起動できません"
            alert.informativeText = "\(editor.path)\n\(error.localizedDescription)"
            alert.runModal()
        }
    }

    /// 渡し終えたら終わる。続けて届いたら延ばす
    private func scheduleQuit() {
        quit?.cancel()
        let item = DispatchWorkItem { NSApp.terminate(nil) }
        quit = item
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.0, execute: item)
    }
}

let application = NSApplication.shared
let launcher = Launcher()
application.delegate = launcher
application.run()
