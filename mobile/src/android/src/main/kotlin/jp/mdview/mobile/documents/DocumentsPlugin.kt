// mdview のモバイル版の、端末の書類（SAF）・出力・共有（v3 R-02）。Rust（src/native.rs）から JNI で呼ぶ。
//
// - **どの呼び出しも待たない**（選択の画面を出したらすぐ返る）。結果は poll() で受け取る
// - 中身はキャッシュのファイルで受け渡す（JNI で渡せるのは文字列だけなので）。開いた書類はキャッシュへ写して道を返し、
//   書き戻すときは Rust が書いたキャッシュのファイルを書類へ写す
// - 開いた書類は書き込みの権限を持ち続ける（takePersistableUriPermission。開き直しても上書きできる）
package jp.mdview.mobile.documents

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import androidx.activity.ComponentActivity
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.FileProvider
import org.json.JSONObject
import java.io.File
import kotlin.concurrent.thread

class DocumentsPlugin(private val activity: Activity) {
  /** 選択の結果（JSON）。まだなら null。poll() で 1 度だけ渡す */
  @Volatile private var result: String? = null

  /** 出力の画面へ渡す、書き出したファイル */
  @Volatile private var pendingExport: File? = null

  private var openLauncher: ActivityResultLauncher<Array<String>>? = null
  private val exportLaunchers = HashMap<String, ActivityResultLauncher<String>>()

  private fun finish(kind: String, vararg pairs: Pair<String, String>) {
    val json = JSONObject()
    json.put("kind", kind)
    for ((key, value) in pairs) json.put(key, value)
    result = json.toString()
  }

  private fun fail(error: Throwable) = finish("failed", "error" to (error.message ?: error.toString()))

  private fun registry() = (activity as ComponentActivity).activityResultRegistry

  private fun folder(name: String): File {
    val dir = File(activity.cacheDir, "mdview/$name")
    dir.mkdirs()
    return dir
  }

  /** Rust が一時のファイルを書く所 */
  fun cacheDir(): String = folder("work").absolutePath

  /** 端末の書類の選択を出す（結果は poll）。失敗したら理由を返す */
  fun pickOpen(): String {
    result = null
    return onUi {
      val launcher = openLauncher ?: registry().register("mdview-open", ActivityResultContracts.OpenDocument()) { uri ->
        if (uri == null) finish("cancelled") else thread { copyIn(uri) }
      }.also { openLauncher = it }
      // .md を text/markdown と名乗らない書類の画面が多いので、絞らない
      launcher.launch(arrayOf("*/*"))
    }
  }

  private fun copyIn(uri: Uri) {
    try {
      val flags = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
      try {
        activity.contentResolver.takePersistableUriPermission(uri, flags)
      } catch (_: SecurityException) {
        // 書き込みを許さない書類（読むだけ）。保存のときに理由が出る
      }
      val name = displayName(uri)
      val target = File(folder("open"), "document")
      activity.contentResolver.openInputStream(uri).use { input ->
        requireNotNull(input) { "書類を読めません" }
        target.outputStream().use { input.copyTo(it) }
      }
      finish("opened", "name" to name, "uri" to uri.toString(), "path" to target.absolutePath)
    } catch (error: Throwable) {
      fail(error)
    }
  }

  private fun displayName(uri: Uri): String {
    activity.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
      if (cursor.moveToFirst()) {
        val name = cursor.getString(0)
        if (!name.isNullOrEmpty()) return name
      }
    }
    return uri.lastPathSegment ?: "document.md"
  }

  /** `{"uri", "path"}`: `path` の中身を書類 `uri` へ書き戻す。できたら空、できなければ理由 */
  fun writeBack(request: String): String = try {
    val json = JSONObject(request)
    val target = Uri.parse(json.getString("uri"))
    val bytes = File(json.getString("path")).readBytes()
    // "wt" は切り詰めて書く。対応しない書類の画面があるので、そのときは "w"
    val stream = try {
      activity.contentResolver.openOutputStream(target, "wt")
    } catch (_: Exception) {
      activity.contentResolver.openOutputStream(target, "w")
    }
    requireNotNull(stream) { "書類へ書けません" }.use { it.write(bytes) }
    ""
  } catch (error: Throwable) {
    error.message ?: error.toString()
  }

  /** `{"name", "mime", "path"}`: 出力の先を選ぶ画面を出し、`path` を写す（結果は poll） */
  fun pickExport(request: String): String {
    val json = JSONObject(request)
    val name = json.getString("name")
    val mime = json.getString("mime")
    result = null
    pendingExport = File(json.getString("path"))
    return onUi {
      val launcher = exportLaunchers.getOrPut(mime) {
        registry().register("mdview-export-$mime", ActivityResultContracts.CreateDocument(mime)) { uri ->
          val source = pendingExport
          if (uri == null || source == null) {
            finish("cancelled")
          } else {
            thread {
              try {
                activity.contentResolver.openOutputStream(uri, "w").use { out ->
                  requireNotNull(out) { "書けません" }
                  source.inputStream().use { it.copyTo(out) }
                }
                finish("exported", "name" to displayName(uri))
              } catch (error: Throwable) {
                fail(error)
              }
            }
          }
        }
      }
      launcher.launch(name)
    }
  }

  /** `{"name", "mime", "path"}`: `path` をほかのアプリへ渡す（共有の画面） */
  fun share(request: String): String = onUi {
    val json = JSONObject(request)
    val name = json.getString("name")
    val mime = json.getString("mime")
    val path = json.getString("path")
    result = null
    val shared = File(folder("share"), name)
    File(path).copyTo(shared, overwrite = true)
    val uri = FileProvider.getUriForFile(activity, activity.packageName + ".mdview.files", shared)
    val send = Intent(Intent.ACTION_SEND)
      .setType(mime)
      .putExtra(Intent.EXTRA_STREAM, uri)
      .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    activity.startActivity(Intent.createChooser(send, name))
    finish("shared")
  }

  /** リンクを開く（ブラウザなど） */
  fun openLink(href: String): String = onUi {
    activity.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(href)))
  }

  /** 選択の結果（JSON）。まだなら空 */
  fun poll(): String {
    val taken = result ?: return ""
    result = null
    return taken
  }

  /** 画面の糸で動かす。**待たない**（Rust の側は画面の糸でないことがある） */
  private fun onUi(block: () -> Unit): String {
    activity.runOnUiThread {
      try {
        block()
      } catch (error: Throwable) {
        fail(error)
      }
    }
    return ""
  }
}
