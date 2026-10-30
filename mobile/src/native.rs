//! 端末の書類・出力・共有を、Android（Kotlin）と iOS（Swift）の部品につなぐ（v3 R-02）。
//!
//! 部品は `manganis::ffi` で APK・.app に同梱する（Kotlin は src/android、Swift は src/ios/plugin）。
//! - **Android は部品のクラスを activity のクラスローダーで読む**。生成されるコードの `FindClass` は、
//!   Rust の糸から呼ぶと system のクラスローダーを使い、アプリのクラスが見つからずに落ちる（既知の問題）
//! - 中身はキャッシュのファイルで受け渡す（渡せるのは文字列だけ）
//! - **引数は 1 つ（JSON）にする**。manganis 0.7.10 は、引数が 2 つ以上の Swift の関数で objc2 が受け付けない呼び方を作る

#![cfg(any(target_os = "android", target_os = "ios"))]

use std::sync::OnceLock;

#[cfg(target_os = "android")]
#[manganis::ffi("src/android")]
extern "Kotlin" {
    pub type DocumentsPlugin;
    pub fn cache_dir(this: &DocumentsPlugin) -> String;
    pub fn pick_open(this: &DocumentsPlugin) -> String;
    pub fn write_back(this: &DocumentsPlugin, request: String) -> String;
    pub fn pick_export(this: &DocumentsPlugin, request: String) -> String;
    pub fn share(this: &DocumentsPlugin, request: String) -> String;
    pub fn open_link(this: &DocumentsPlugin, href: String) -> String;
    pub fn poll(this: &DocumentsPlugin) -> String;
}

#[cfg(target_os = "ios")]
#[manganis::ffi("src/ios/plugin")]
extern "Swift" {
    pub type DocumentsPlugin;
    pub fn cache_dir(this: &DocumentsPlugin) -> String;
    pub fn pick_open(this: &DocumentsPlugin) -> String;
    pub fn write_back(this: &DocumentsPlugin, request: String) -> String;
    pub fn pick_export(this: &DocumentsPlugin, request: String) -> String;
    pub fn share(this: &DocumentsPlugin, request: String) -> String;
    pub fn open_link(this: &DocumentsPlugin, href: String) -> String;
    pub fn poll(this: &DocumentsPlugin) -> String;
}

/// 部品の名前（Kotlin の build.gradle.kts の namespace ＋ クラス名）
#[cfg(target_os = "android")]
const CLASS: &str = "jp.mdview.mobile.documents.DocumentsPlugin";

/// 部品は 1 つだけ作る
struct Shared(DocumentsPlugin);

// SAFETY: Android は JNI の大域参照（どの糸からでも使える）、iOS は Objective-C の物で、部品の中は錠で守っている
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

static PLUGIN: OnceLock<Result<Shared, String>> = OnceLock::new();

pub fn plugin() -> Result<&'static DocumentsPlugin, String> {
    PLUGIN
        .get_or_init(|| create().map(Shared))
        .as_ref()
        .map(|shared| &shared.0)
        .map_err(Clone::clone)
}

#[cfg(target_os = "ios")]
fn create() -> Result<DocumentsPlugin, String> {
    DocumentsPlugin::new().map_err(str::to_owned)
}

#[cfg(target_os = "android")]
fn create() -> Result<DocumentsPlugin, String> {
    use manganis::jni::objects::{JClass, JObject, JValue};

    manganis::android::with_activity(|env, activity| {
        let made = (|| -> Result<_, manganis::jni::errors::Error> {
            let loader = env
                .call_method(activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
                .l()?;
            let name = env.new_string(CLASS)?;
            let class: JClass = env
                .call_method(
                    &loader,
                    "loadClass",
                    "(Ljava/lang/String;)Ljava/lang/Class;",
                    &[JValue::Object(&JObject::from(name))],
                )?
                .l()?
                .into();
            let instance = env.new_object(
                &class,
                "(Landroid/app/Activity;)V",
                &[JValue::Object(activity)],
            )?;
            env.new_global_ref(instance)
        })();
        // Java の例外が残っていると、次の JNI の呼び出しが落ちる
        if made.is_err() && env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
        Some(
            made.map(DocumentsPlugin::from_global_ref)
                .map_err(|error| format!("{error:?}")),
        )
    })
    .unwrap_or_else(|| Err("Android の画面を取れません".into()))
    .map_err(|error| format!("端末の部品を読めません: {error}"))
}

/// 部品の返事。**空なら成功**、そうでなければ理由
pub fn ok(reply: Result<String, impl ToString>) -> Result<(), String> {
    match reply.map_err(|error| error.to_string())? {
        reply if reply.is_empty() => Ok(()),
        reason => Err(reason),
    }
}

/// 部品の返事（文字列そのもの）
pub fn text(reply: Result<String, impl ToString>) -> Result<String, String> {
    reply.map_err(|error| error.to_string())
}
