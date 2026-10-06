//! 埋め込み（図・数式・画像）の非同期描画（§4.3 / §16.5）。
//!
//! 図や数式は 1 件あたり数百 ms〜秒単位になり得る。**UI スレッドで描いてはいけない。**
//! ここではワーカープールと、結果を受け取るまでの「未確定」の扱いを用意する。
//!
//! **iced を知らない。** レイアウト層と同じく、ウィンドウ無しで試験できる
//! （§4.2）。実際の描画器（merman / latex-rust / 画像デコーダ）は
//! [`RenderEmbed`] として外から挿す。

pub mod diagram;
mod image;
mod math;
mod pool;

// EmbedPool はアプリ層が持つ。JobState はレイアウト層が箱の大きさを決めるのに使う
#[allow(unused_imports)]
pub use diagram::DiagramRenderer;
#[allow(unused_imports)]
pub use image::{is_remote, resolve, stamp_of, ImageRenderer};
#[allow(unused_imports)]
pub use math::MathRenderer;
#[allow(unused_imports)]
pub use pool::{EmbedPool, JobState};

use std::hash::{Hash, Hasher};

/// 埋め込みの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EmbedKind {
    /// Mermaid の図
    Diagram,
    /// LaTeX の数式（別行立て）
    Math,
    /// 画像
    Image,
}

/// 描画の依頼。
#[derive(Debug, Clone, PartialEq)]
pub struct EmbedSource {
    pub kind: EmbedKind,
    /// 図と数式は本文、画像は参照先の文字列
    pub text: String,
    /// 利用可能幅（px）。図は幅に合わせて描くため、幅が変われば描き直す
    pub width: f32,
    /// 参照先の更新時刻（ミリ秒）。**画像だけが持つ**（DD-OPEN-16）。
    ///
    /// 図と数式は本文そのものが鍵になるので要らない。画像はパスしか
    /// 鍵に入らず、**中身を差し替えても同じ鍵になってしまう**
    pub stamp: Option<u64>,
}

impl EmbedSource {
    pub fn new(kind: EmbedKind, text: impl Into<String>, width: f32) -> Self {
        Self {
            kind,
            text: text.into(),
            width,
            stamp: None,
        }
    }

    /// 参照先の更新時刻を添える（画像用）。
    pub fn with_stamp(mut self, stamp: Option<u64>) -> Self {
        self.stamp = stamp;
        self
    }

    /// この依頼を一意に指す鍵。
    pub fn key(&self) -> EmbedKey {
        EmbedKey::of(self)
    }
}

/// 結果を引くための鍵。
///
/// **添字やブロック番号を使ってはいけない。** 編集でブロックが増減すると
/// 別の内容が同じ鍵を取り、古い図が出る。レイアウトキャッシュ（§3.8）で
/// 同じ誤りを踏んでいる。
///
/// 内容から作るので、**編集で内容が変われば鍵も変わる**。
/// 結果が遅れて届いても、古い鍵の結果は誰も引かないだけで害がない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EmbedKey {
    pub kind: EmbedKind,
    hash: u64,
    /// 幅。**0.5px 単位へ量子化する**（レイアウトキャッシュと同じ理由）
    width_q: u32,
}

impl EmbedKey {
    fn of(source: &EmbedSource) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        source.kind.hash(&mut hasher);
        source.text.hash(&mut hasher);
        // **更新時刻も混ぜる。** 同じパスでも中身が変われば別の鍵になる
        source.stamp.hash(&mut hasher);
        Self {
            kind: source.kind,
            hash: hasher.finish(),
            width_q: (source.width.max(0.0) * 2.0).round() as u32,
        }
    }
}

/// 描き上がったもの。
///
/// **画素で持つ。** SVG のまま持つと描画のたびにラスタライズが要る。
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedEmbed {
    pub width: u32,
    pub height: u32,
    /// RGBA8。`width * height * 4` バイト
    pub pixels: std::sync::Arc<Vec<u8>>,
}

impl RenderedEmbed {
    /// 表示上の高さ（px）。幅に合わせて縮めたときの値を返す。
    pub fn display_height(&self, available: f32) -> f32 {
        if self.width == 0 || self.height == 0 {
            return 0.0;
        }
        let scale = (available / self.width as f32).min(1.0);
        self.height as f32 * scale
    }
}

/// 描けなかった理由。**握りつぶさない。** 画面に出して利用者へ伝える。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbedError {
    /// まだ実装していない種類
    Unsupported(EmbedKind),
    /// 描画器が失敗した（構文誤りなど）
    Failed(String),
}

impl std::fmt::Display for EmbedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(kind) => write!(f, "未対応: {kind:?}"),
            Self::Failed(reason) => write!(f, "描画に失敗: {reason}"),
        }
    }
}

/// 実際に描くもの。ワーカースレッドで呼ばれる。
///
/// **`Send + Sync` である。** 複数のワーカーが同時に呼ぶ。
pub trait RenderEmbed: Send + Sync {
    fn render(&self, source: &EmbedSource) -> Result<RenderedEmbed, EmbedError>;
}

/// まだ描画器を挿していない状態。
///
/// **黙って何も出さないのではなく、未対応であることを画面に出す。**
/// P3 の以降の段階で merman / latex-rust / 画像デコーダに差し替える。
pub struct NotImplemented;

impl RenderEmbed for NotImplemented {
    fn render(&self, source: &EmbedSource) -> Result<RenderedEmbed, EmbedError> {
        Err(EmbedError::Unsupported(source.kind))
    }
}

/// 種類ごとに描画器を振り分ける。
///
/// **1 つの描画器に全部渡してはいけない。** 数式を merman へ渡して
/// 「No Mermaid diagram type detected」と出した（実際に踏んだ）。
/// 未実装の種類は、その種類として「未対応」と返す。
pub struct Dispatch {
    diagram: Option<Box<dyn RenderEmbed>>,
    math: Option<Box<dyn RenderEmbed>>,
    image: Option<Box<dyn RenderEmbed>>,
}

impl Dispatch {
    pub fn new() -> Self {
        Self {
            diagram: None,
            math: None,
            image: None,
        }
    }

    pub fn with_diagram(mut self, renderer: impl RenderEmbed + 'static) -> Self {
        self.diagram = Some(Box::new(renderer));
        self
    }

    pub fn with_math(mut self, renderer: impl RenderEmbed + 'static) -> Self {
        self.math = Some(Box::new(renderer));
        self
    }

    pub fn with_image(mut self, renderer: impl RenderEmbed + 'static) -> Self {
        self.image = Some(Box::new(renderer));
        self
    }
}

impl Default for Dispatch {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderEmbed for Dispatch {
    fn render(&self, source: &EmbedSource) -> Result<RenderedEmbed, EmbedError> {
        let renderer = match source.kind {
            EmbedKind::Diagram => &self.diagram,
            EmbedKind::Math => &self.math,
            EmbedKind::Image => &self.image,
        };
        match renderer {
            Some(renderer) => renderer.render(source),
            None => Err(EmbedError::Unsupported(source.kind)),
        }
    }
}

/// 結果が届くまでに置く高さ（§16.5）。
pub const PLACEHOLDER_HEIGHT: f32 = 240.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_follows_the_content() {
        let a = EmbedSource::new(EmbedKind::Diagram, "graph TD; A-->B", 800.0);
        let b = EmbedSource::new(EmbedKind::Diagram, "graph TD; A-->B", 800.0);
        let c = EmbedSource::new(EmbedKind::Diagram, "graph TD; A-->C", 800.0);
        assert_eq!(a.key(), b.key());
        assert_ne!(a.key(), c.key(), "内容が違えば別の鍵");
    }

    /// **同じパスでも、中身が変われば別の鍵になる**（DD-OPEN-16）。
    ///
    /// これが無いと、画像を差し替えても古いものが出続ける
    #[test]
    fn key_follows_the_stamp() {
        let path = "C:/tmp/a.png";
        let old = EmbedSource::new(EmbedKind::Image, path, 800.0).with_stamp(Some(1_000));
        let new = EmbedSource::new(EmbedKind::Image, path, 800.0).with_stamp(Some(2_000));
        let same = EmbedSource::new(EmbedKind::Image, path, 800.0).with_stamp(Some(1_000));
        assert_ne!(old.key(), new.key(), "差し替えに気づいていない");
        assert_eq!(old.key(), same.key());
    }

    /// 更新時刻を読めなかった場合も、鍵としては成り立つ。
    #[test]
    fn a_missing_stamp_is_its_own_key() {
        let path = "C:/tmp/a.png";
        let unknown = EmbedSource::new(EmbedKind::Image, path, 800.0);
        let known = EmbedSource::new(EmbedKind::Image, path, 800.0).with_stamp(Some(1_000));
        assert_ne!(unknown.key(), known.key());
    }

    #[test]
    fn key_follows_the_kind() {
        let diagram = EmbedSource::new(EmbedKind::Diagram, "x", 800.0);
        let math = EmbedSource::new(EmbedKind::Math, "x", 800.0);
        assert_ne!(diagram.key(), math.key());
    }

    /// 幅は 0.5px 単位へ量子化する（リサイズ中の取りこぼしを避ける）。
    #[test]
    fn width_is_quantised() {
        let a = EmbedSource::new(EmbedKind::Diagram, "x", 800.0);
        let b = EmbedSource::new(EmbedKind::Diagram, "x", 800.2);
        let c = EmbedSource::new(EmbedKind::Diagram, "x", 801.0);
        assert_eq!(a.key(), b.key());
        assert_ne!(a.key(), c.key());
    }

    /// 幅に収まるなら原寸、超えるなら縦横比を保って縮める（§3.2 の箱の扱い）。
    #[test]
    fn display_height_keeps_the_aspect_ratio() {
        let embed = RenderedEmbed {
            width: 400,
            height: 200,
            pixels: std::sync::Arc::new(Vec::new()),
        };
        assert_eq!(embed.display_height(800.0), 200.0, "原寸のまま");
        assert_eq!(embed.display_height(200.0), 100.0, "半分に縮む");
    }

    #[test]
    fn zero_sized_embed_is_safe() {
        let embed = RenderedEmbed {
            width: 0,
            height: 0,
            pixels: std::sync::Arc::new(Vec::new()),
        };
        assert_eq!(embed.display_height(800.0), 0.0);
    }

    /// **種類ごとに振り分ける。** 数式を図の描画器へ渡さない。
    #[test]
    fn dispatch_routes_by_kind() {
        struct Always;
        impl RenderEmbed for Always {
            fn render(&self, _source: &EmbedSource) -> Result<RenderedEmbed, EmbedError> {
                Ok(RenderedEmbed {
                    width: 1,
                    height: 1,
                    pixels: std::sync::Arc::new(vec![0; 4]),
                })
            }
        }

        let dispatch = Dispatch::new().with_diagram(Always);
        assert!(dispatch
            .render(&EmbedSource::new(EmbedKind::Diagram, "x", 800.0))
            .is_ok());
        // 数式は未実装 = 図の描画器へ渡さず、数式として未対応を返す
        assert_eq!(
            dispatch
                .render(&EmbedSource::new(EmbedKind::Math, "x", 800.0))
                .unwrap_err(),
            EmbedError::Unsupported(EmbedKind::Math)
        );
        assert_eq!(
            dispatch
                .render(&EmbedSource::new(EmbedKind::Image, "x", 800.0))
                .unwrap_err(),
            EmbedError::Unsupported(EmbedKind::Image)
        );
    }

    /// 未実装の描画器は**黙らず**未対応を返す。
    #[test]
    fn not_implemented_reports_the_kind() {
        let source = EmbedSource::new(EmbedKind::Math, "x^2", 800.0);
        let error = NotImplemented.render(&source).unwrap_err();
        assert_eq!(error, EmbedError::Unsupported(EmbedKind::Math));
        assert!(error.to_string().contains("未対応"));
    }
}
