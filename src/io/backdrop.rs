//! 背景画像（v2.4.0 R-01）。
//!
//! **好きな画像を本文の後ろに敷く。** ここでは設定（敷く所・表示方法・位置・透明度・ぼかし・
//! 動き方・明るい／暗い配色ごとの画像）と、画像の読み込み（縮める・ぼかす）を持つ。描くのは
//! `crate::render::backdrop`。
//!
//! 設定ファイルには `bg_` で始まる鍵で 1 項目 1 行に書く。項目ごとに書くので、窓が複数あっても
//! 変えた項目だけが重なる（`Settings::merge_changes`）。

use std::path::{Path, PathBuf};

/// 表示範囲（設定画面の並びは面積の広い順）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Area {
    /// 編集画面とプレビュー（それぞれの領域に 1 枚ずつ）
    #[default]
    Panes,
    /// 窓全体（メニュー・目次・ステータスバーを含む）
    Window,
    /// 編集画面だけ
    Editor,
    /// プレビューだけ
    Preview,
    /// 分割表示の 2 つの領域にまたがって 1 枚
    Span,
}

impl Area {
    /// **面積の広い順**（設定画面の並び）
    pub const ALL: [Area; 5] = [
        Self::Window,
        Self::Span,
        Self::Panes,
        Self::Editor,
        Self::Preview,
    ];

    /// 設定ファイルに書く名前
    pub fn key(self) -> &'static str {
        match self {
            Self::Panes => "panes",
            Self::Window => "window",
            Self::Editor => "editor",
            Self::Preview => "preview",
            Self::Span => "span",
        }
    }

    fn parse(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|area| area.key() == value.trim())
            .unwrap_or_default()
    }
}

impl std::fmt::Display for Area {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Panes => "編集画面とプレビュー（それぞれ）",
            Self::Window => "ウィンドウ全体",
            Self::Editor => "編集画面のみ",
            Self::Preview => "プレビューのみ",
            Self::Span => "編集画面とプレビュー（全体）",
        })
    }
}

/// 表示方法
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fit {
    /// 並べる（繰り返す）
    Tile,
    /// 伸ばす（縦横比を保たない）
    Stretch,
    /// 全体を収める（縦横比を保ち、全体が入る）
    Contain,
    /// 覆う（縦横比を保ち、隙間なく覆う。はみ出しは切る）
    #[default]
    Cover,
    /// 原寸で置く
    Original,
}

impl Fit {
    pub const ALL: [Fit; 5] = [
        Self::Tile,
        Self::Stretch,
        Self::Contain,
        Self::Cover,
        Self::Original,
    ];

    /// 設定ファイルに書く名前
    pub fn key(self) -> &'static str {
        match self {
            Self::Tile => "tile",
            Self::Stretch => "stretch",
            Self::Contain => "contain",
            Self::Cover => "cover",
            Self::Original => "original",
        }
    }

    fn parse(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|fit| fit.key() == value.trim())
            .unwrap_or_default()
    }
}

impl std::fmt::Display for Fit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Tile => "並べる（繰り返す）",
            Self::Stretch => "伸ばす（縦横比を保たない）",
            Self::Contain => "全体を収める",
            Self::Cover => "覆う（はみ出しは切る）",
            Self::Original => "原寸で置く",
        })
    }
}

/// 配置（9 か所）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    #[default]
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Anchor {
    pub const ALL: [Anchor; 9] = [
        Self::TopLeft,
        Self::Top,
        Self::TopRight,
        Self::Left,
        Self::Center,
        Self::Right,
        Self::BottomLeft,
        Self::Bottom,
        Self::BottomRight,
    ];

    /// 設定ファイルに書く名前
    pub fn key(self) -> &'static str {
        match self {
            Self::TopLeft => "top_left",
            Self::Top => "top",
            Self::TopRight => "top_right",
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
            Self::BottomLeft => "bottom_left",
            Self::Bottom => "bottom",
            Self::BottomRight => "bottom_right",
        }
    }

    fn parse(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|anchor| anchor.key() == value.trim())
            .unwrap_or_default()
    }

    /// 横・縦の寄せ（0 = 左・上、0.5 = 中、1 = 右・下）
    pub fn factors(self) -> (f32, f32) {
        let index = Self::ALL.iter().position(|a| *a == self).unwrap_or(4);
        ((index % 3) as f32 / 2.0, (index / 3) as f32 / 2.0)
    }
}

impl std::fmt::Display for Anchor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::TopLeft => "左上",
            Self::Top => "上",
            Self::TopRight => "右上",
            Self::Left => "左",
            Self::Center => "中央",
            Self::Right => "右",
            Self::BottomLeft => "左下",
            Self::Bottom => "下",
            Self::BottomRight => "右下",
        })
    }
}

/// ぼかしの強さ
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Blur {
    #[default]
    None,
    Weak,
    Medium,
    Strong,
}

impl Blur {
    pub const ALL: [Blur; 4] = [Self::None, Self::Weak, Self::Medium, Self::Strong];

    /// 設定ファイルに書く名前
    pub fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Weak => "weak",
            Self::Medium => "medium",
            Self::Strong => "strong",
        }
    }

    fn parse(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|blur| blur.key() == value.trim())
            .unwrap_or_default()
    }

    /// ぼかす量（縮めたあとの画像での標準偏差。px）
    fn sigma(self) -> f32 {
        match self {
            Self::None => 0.0,
            Self::Weak => 3.0,
            Self::Medium => 8.0,
            Self::Strong => 16.0,
        }
    }
}

impl std::fmt::Display for Blur {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::None => "無し",
            Self::Weak => "弱",
            Self::Medium => "中",
            Self::Strong => "強",
        })
    }
}

/// 配色ごとの見た目（明るい配色・暗い配色で別々に持つ）
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    /// 画像ファイル（空なら敷かない。暗い配色で空なら明るい配色の画像を使う）
    pub image: String,
    /// 不透明の度合い（%。0 = 見えない、100 = そのまま）
    pub opacity: u8,
    pub blur: Blur,
}

/// **既定の濃さ。** 文字が読める薄さ（本文の地の上に重ねる）
pub const DEFAULT_OPACITY: u8 = 20;

impl Default for Look {
    fn default() -> Self {
        Self {
            image: String::new(),
            opacity: DEFAULT_OPACITY,
            blur: Blur::None,
        }
    }
}

/// 背景画像の設定
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Backdrop {
    /// 使うか（既定は使わない）
    pub enabled: bool,
    pub area: Area,
    pub fit: Fit,
    pub anchor: Anchor,
    /// 本文と一緒に動かすか（既定は窓に固定）
    pub scroll: bool,
    pub light: Look,
    pub dark: Look,
}

/// 設定ファイルの鍵（**設定画面と同じ並び**）
pub const KEYS: [&str; 11] = [
    "bg_enabled",
    "bg_area",
    "bg_fit",
    "bg_anchor",
    "bg_scroll",
    "bg_light_image",
    "bg_light_opacity",
    "bg_light_blur",
    "bg_dark_image",
    "bg_dark_opacity",
    "bg_dark_blur",
];

/// 設定ファイルに書けるか（`"` と改行は読み戻せない）
fn quotable(text: &str) -> bool {
    !text.contains('"') && !text.contains('\n') && !text.contains('\r')
}

impl Backdrop {
    /// 設定ファイルの行（`KEYS` の順）
    pub fn to_toml(&self) -> String {
        let mut out = format!(
            "bg_enabled = {}\nbg_area = \"{}\"\nbg_fit = \"{}\"\nbg_anchor = \"{}\"\nbg_scroll = {}\n",
            self.enabled,
            self.area.key(),
            self.fit.key(),
            self.anchor.key(),
            self.scroll,
        );
        for (name, look) in [("light", &self.light), ("dark", &self.dark)] {
            if quotable(&look.image) {
                out.push_str(&format!("bg_{name}_image = \"{}\"\n", look.image));
            }
            out.push_str(&format!(
                "bg_{name}_opacity = {}\nbg_{name}_blur = \"{}\"\n",
                look.opacity,
                look.blur.key()
            ));
        }
        out
    }

    /// 1 項目を読む（`value` は引用符を外したもの）。**読めない値は変えない**。知らない鍵なら偽
    pub fn set(&mut self, key: &str, value: &str) -> bool {
        let flag = match value.trim() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        };
        match key {
            "bg_enabled" => {
                if let Some(on) = flag {
                    self.enabled = on;
                }
            }
            "bg_scroll" => {
                if let Some(on) = flag {
                    self.scroll = on;
                }
            }
            "bg_area" => self.area = Area::parse(value),
            "bg_fit" => self.fit = Fit::parse(value),
            "bg_anchor" => self.anchor = Anchor::parse(value),
            _ => {
                let Some(rest) = key.strip_prefix("bg_") else {
                    return false;
                };
                let (look, field) = match rest.split_once('_') {
                    Some(("light", field)) => (&mut self.light, field),
                    Some(("dark", field)) => (&mut self.dark, field),
                    _ => return false,
                };
                match field {
                    "image" => {
                        if quotable(value) {
                            look.image = value.trim().to_owned();
                        }
                    }
                    "opacity" => {
                        if let Ok(opacity) = value.trim().parse::<u8>() {
                            look.opacity = opacity.min(100);
                        }
                    }
                    "blur" => look.blur = Blur::parse(value),
                    _ => return false,
                }
            }
        }
        true
    }

    /// いまの配色で使う見た目。**暗い配色で画像が空なら、明るい配色の画像を使う**（透明度とぼかしは暗い配色のもの）
    pub fn look(&self, dark: bool) -> Look {
        if !dark {
            return self.light.clone();
        }
        let mut look = self.dark.clone();
        if look.image.trim().is_empty() {
            look.image.clone_from(&self.light.image);
        }
        look
    }

    /// 読み込む画像（使わない・画像が空なら `None`）
    pub fn wanted(&self, dark: bool) -> Option<Wanted> {
        if !self.enabled {
            return None;
        }
        let look = self.look(dark);
        let path = look.image.trim();
        if path.is_empty() {
            return None;
        }
        Some(Wanted {
            path: PathBuf::from(path),
            blur: look.blur,
        })
    }
}

/// 読み込む画像の鍵（同じなら読み直さない）
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Wanted {
    pub path: PathBuf,
    pub blur: Blur,
}

/// 読み込んだ画像（縮めてぼかしたあとの RGBA）
#[derive(Debug, Clone)]
pub struct Loaded {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// **縮める上限**（長い辺の画素）。画面より大きい画像は描く負担とメモリが増えるだけ
pub const MAX_SIDE: u32 = 2560;

/// 画像を読み、長い辺を `MAX_SIDE` まで縮め、ぼかす。**描くたびにはしない**（読み込むときに 1 度）
pub fn load(wanted: &Wanted) -> Result<Loaded, String> {
    let image = image::open(&wanted.path).map_err(|error| describe(&wanted.path, &error))?;
    Ok(prepare(image, wanted.blur))
}

fn describe(path: &Path, error: &image::ImageError) -> String {
    match error {
        image::ImageError::IoError(io) if io.kind() == std::io::ErrorKind::NotFound => {
            format!("背景画像が見つかりません: {}", path.display())
        }
        _ => format!("背景画像を読めません（{}）: {error}", path.display()),
    }
}

/// 縮めて・ぼかして RGBA にする
fn prepare(image: image::DynamicImage, blur: Blur) -> Loaded {
    let image = if image.width().max(image.height()) > MAX_SIDE {
        image.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    let mut rgba = image.to_rgba8();
    let sigma = blur.sigma();
    if sigma > 0.0 {
        rgba = image::imageops::fast_blur(&rgba, sigma);
    }
    Loaded {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    }
}

/// 画像を置く矩形（`bounds` は描く領域、`size` は画像の大きさ、`offset_y` は本文と一緒に動かすときの送り量）。
///
/// **並べるとき**は、位置を基準に必要な枚数だけ返す。ほかは 1 枚（はみ出しは描く側で切る）
pub fn placements(
    bounds: (f32, f32, f32, f32),
    size: (f32, f32),
    fit: Fit,
    anchor: Anchor,
    offset_y: f32,
) -> Vec<(f32, f32, f32, f32)> {
    let (x, y, width, height) = bounds;
    let (image_w, image_h) = size;
    if image_w <= 0.0 || image_h <= 0.0 || width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let (fx, fy) = anchor.factors();
    let (w, h) = match fit {
        Fit::Stretch => (width, height),
        Fit::Contain => {
            let scale = (width / image_w).min(height / image_h);
            (image_w * scale, image_h * scale)
        }
        Fit::Cover => {
            let scale = (width / image_w).max(height / image_h);
            (image_w * scale, image_h * scale)
        }
        Fit::Tile | Fit::Original => (image_w, image_h),
    };
    let left = x + (width - w) * fx;
    let top = y + (height - h) * fy - offset_y;
    if fit != Fit::Tile {
        return vec![(left, top, w, h)];
    }
    // 位置を基準に、領域を覆うだけ並べる
    let first_x = left - ((left - x) / w).ceil() * w;
    let first_y = top - ((top - y) / h).ceil() * h;
    let mut out = Vec::new();
    let mut ty = first_y;
    while ty < y + height {
        let mut tx = first_x;
        while tx < x + width {
            out.push((tx, ty, w, h));
            tx += w;
        }
        ty += h;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 設定は書いて読むと戻る（明るい・暗いで別々）
    #[test]
    fn round_trip() {
        let backdrop = Backdrop {
            enabled: true,
            area: Area::Span,
            fit: Fit::Tile,
            anchor: Anchor::BottomRight,
            scroll: true,
            light: Look {
                image: "C:\\画像\\昼.png".to_owned(),
                opacity: 35,
                blur: Blur::Weak,
            },
            dark: Look {
                image: "/home/me/夜.jpg".to_owned(),
                opacity: 10,
                blur: Blur::Strong,
            },
        };
        let mut read = Backdrop::default();
        for line in backdrop.to_toml().lines() {
            let (key, value) = line.split_once('=').unwrap();
            assert!(
                read.set(key.trim(), value.trim().trim_matches('"')),
                "{key}"
            );
        }
        assert_eq!(read, backdrop);
        assert_eq!(
            backdrop
                .to_toml()
                .lines()
                .map(|l| l.split_once(" =").unwrap().0)
                .collect::<Vec<_>>(),
            KEYS,
            "鍵の並びが KEYS と違う"
        );
    }

    /// **読めない値は変えない**（その項目だけ既定のまま）
    #[test]
    fn unreadable_values_keep_the_default() {
        let mut backdrop = Backdrop::default();
        assert!(backdrop.set("bg_enabled", "maybe"));
        assert!(backdrop.set("bg_light_opacity", "-5"));
        assert!(backdrop.set("bg_fit", "zoom"));
        assert_eq!(backdrop, Backdrop::default());
        assert!(backdrop.set("bg_dark_opacity", "250"));
        assert_eq!(backdrop.dark.opacity, 100, "上限で止める");
        assert!(!backdrop.set("bg_middle_image", "x"));
    }

    /// 暗い配色で画像が空なら、明るい配色の画像を使う
    #[test]
    fn the_dark_look_falls_back_to_the_light_image() {
        let mut backdrop = Backdrop {
            enabled: true,
            ..Backdrop::default()
        };
        backdrop.light.image = "a.png".to_owned();
        backdrop.dark.opacity = 5;
        let look = backdrop.look(true);
        assert_eq!((look.image.as_str(), look.opacity), ("a.png", 5));
        assert!(backdrop.wanted(true).is_some());
        backdrop.enabled = false;
        assert!(backdrop.wanted(false).is_none(), "使わないなら読まない");
    }

    /// 表示方法ごとの置き方
    #[test]
    fn placements_follow_the_fit() {
        let area = (0.0, 0.0, 400.0, 200.0);
        let image = (100.0, 100.0);
        assert_eq!(
            placements(area, image, Fit::Stretch, Anchor::Center, 0.0),
            vec![(0.0, 0.0, 400.0, 200.0)]
        );
        assert_eq!(
            placements(area, image, Fit::Contain, Anchor::Center, 0.0),
            vec![(100.0, 0.0, 200.0, 200.0)],
            "縦に合わせて中央"
        );
        assert_eq!(
            placements(area, image, Fit::Cover, Anchor::TopLeft, 0.0),
            vec![(0.0, 0.0, 400.0, 400.0)],
            "横に合わせ、下がはみ出す"
        );
        assert_eq!(
            placements(area, image, Fit::Original, Anchor::BottomRight, 0.0),
            vec![(300.0, 100.0, 100.0, 100.0)]
        );
        let tiles = placements(area, image, Fit::Tile, Anchor::TopLeft, 0.0);
        assert_eq!(tiles.len(), 8, "4 × 2 枚で覆う");
        // 本文と一緒に動かすと、送った分だけ上へずれる（並べるときは隙間なく覆い続ける）
        let moved = placements(area, image, Fit::Tile, Anchor::TopLeft, 30.0);
        assert!(moved.iter().any(|t| t.1 < 0.0) && moved.iter().all(|t| t.1 < 200.0));
        let covered: f32 = moved.iter().filter(|t| t.0 == 0.0).map(|t| t.3).sum();
        assert!(covered >= 200.0 + 30.0 - 0.1, "縦に隙間がある: {moved:?}");
    }

    /// 大きい画像は縮め、ぼかしても大きさは変わらない
    #[test]
    fn large_images_are_shrunk_once() {
        let image = image::DynamicImage::new_rgba8(MAX_SIDE * 2, 100);
        let loaded = prepare(image, Blur::Medium);
        assert_eq!((loaded.width, loaded.height), (MAX_SIDE, 50));
        assert_eq!(loaded.rgba.len(), (MAX_SIDE * 50 * 4) as usize);
    }
}
