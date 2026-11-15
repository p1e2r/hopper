//! Pixel-style widgets: bevelled buttons, inset slots, switches, pixel icons
//! and the bundled textures. Everything here is drawn from scratch; no game
//! assets are used.

use gpui::{
    App, Div, ElementId, FontWeight, Image, ImageFormat, ObjectFit, SharedString, Stateful, div,
    img, prelude::*, px, rgb,
};
use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

pub const FONT: &str = "Monocraft";

// Palette
pub const BG: u32 = 0x1c1c1f; // deepslate
pub const BAR: u32 = 0x26262a;
pub const BLACK: u32 = 0x0a0a0b;
pub const TEXT: u32 = 0xf0f0f0;
pub const MUTED: u32 = 0x9a9aa3;
pub const GOLD: u32 = 0xffd84a;
pub const GREEN: u32 = 0x6cc349;
pub const GREEN_DARK: u32 = 0x2f5d22;
pub const RED: u32 = 0xff6b6b;

const BTN_FACE: u32 = 0x6f6f76;
const BTN_HOVER: u32 = 0x7f8aa8;
const BTN_HL: u32 = 0xa9a9b1;
const BTN_SH: u32 = 0x3c3c41;

const SLOT_FACE: u32 = 0x2e2e33;
const SLOT_DARK: u32 = 0x161618;
const SLOT_LIGHT: u32 = 0x4a4a51;

pub fn load_fonts(cx: &mut App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!("../assets/fonts/Monocraft.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Monocraft-Bold.ttf")),
    ];
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        eprintln!("couldn't load the pixel font: {e}");
    }
}

fn texture(cell: &'static OnceLock<Arc<Image>>, bytes: &'static [u8]) -> Arc<Image> {
    cell.get_or_init(|| Arc::new(Image::from_bytes(ImageFormat::Png, bytes.to_vec())))
        .clone()
}

/// A grass-and-dirt band for the top of the window.
pub fn grass_strip() -> impl IntoElement {
    static TEX: OnceLock<Arc<Image>> = OnceLock::new();
    let tex = texture(&TEX, include_bytes!("../assets/textures/grass_strip.png"));
    div()
        .w_full()
        .h(px(27.))
        .flex_shrink_0()
        .overflow_hidden()
        .child(img(tex).w(px(1920.)).h(px(27.)).object_fit(ObjectFit::Fill))
}

/// The app icon, as shown in the dock/taskbar.
pub fn app_icon(size: f32) -> impl IntoElement {
    static TEX: OnceLock<Arc<Image>> = OnceLock::new();
    let tex = texture(&TEX, include_bytes!("../assets/icon/icon.png"));
    img(tex)
        .flex_shrink_0()
        .size(px(size))
        .object_fit(ObjectFit::Contain)
}

/// Stone texture placed behind the sidebar.
pub fn stone_backdrop() -> impl IntoElement {
    static TEX: OnceLock<Arc<Image>> = OnceLock::new();
    let tex = texture(&TEX, include_bytes!("../assets/textures/stone.png"));
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .overflow_hidden()
        .child(
            img(tex)
                .w(px(216.))
                .h(px(1680.))
                .object_fit(ObjectFit::Fill),
        )
}

/// Gold title text with a hard drop shadow.
pub fn title(text: impl Into<SharedString>, size: f32) -> impl IntoElement {
    let text = text.into();
    let off = (size / 9.0).round().max(2.0);
    div()
        .relative()
        .flex_shrink_0()
        .text_size(px(size))
        .font_weight(FontWeight::BOLD)
        .child(
            div()
                .absolute()
                .top(px(off))
                .left(px(off))
                .text_color(rgb(0x3f3410))
                .child(text.clone()),
        )
        .child(div().text_color(rgb(GOLD)).child(text))
}

/// Section heading with a shadow, like in-game menus.
pub fn heading(text: impl Into<SharedString>) -> impl IntoElement {
    let text = text.into();
    div()
        .relative()
        .flex_shrink_0()
        .text_size(px(18.))
        .font_weight(FontWeight::BOLD)
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(2.))
                .text_color(rgb(0x2a2a2a))
                .child(text.clone()),
        )
        .child(div().text_color(rgb(TEXT)).child(text))
}

/// A raised, bevelled button. Callers attach `on_click` (and may set a width).
pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    raised(id, BTN_FACE, BTN_HL, BTN_SH, BTN_HOVER)
        .child(label.into())
        .0
}

/// Green variant for the main action.
pub fn primary_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    raised(id, 0x3f8a2a, 0x7fcf5b, 0x1f4a15, 0x4ea235)
        .child(label.into())
        .0
}

/// Gold variant, for "update available".
pub fn gold_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    raised(id, 0xc9a227, 0xffe680, 0x6b5410, 0xdcb43a)
        .child(div().text_color(rgb(0x1a1400)).child(label.into()))
        .0
}

/// A finished raised bevel; `.0` is the clickable outer element.
pub struct Raised(pub Stateful<Div>);

pub fn raised(id: impl Into<ElementId>, face: u32, hl: u32, sh: u32, hover: u32) -> RaisedBuilder {
    RaisedBuilder {
        id: id.into(),
        face,
        hl,
        sh,
        hover,
    }
}

pub struct RaisedBuilder {
    id: ElementId,
    face: u32,
    hl: u32,
    sh: u32,
    hover: u32,
}

impl RaisedBuilder {
    pub fn child(self, content: impl IntoElement) -> Raised {
        let face = div()
            .flex_1()
            .min_h(px(22.))
            .px_3()
            .flex()
            .items_center()
            .justify_center()
            .gap_2()
            .bg(rgb(self.face))
            .hover(move |s| s.bg(rgb(self.hover)))
            .child(content);
        Raised(
            div()
                .id(self.id)
                .flex_shrink_0()
                .flex()
                .p(px(2.))
                .bg(rgb(BLACK))
                .cursor_pointer()
                .text_color(rgb(TEXT))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .pb(px(3.))
                        .pr(px(2.))
                        .bg(rgb(self.sh))
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .pt(px(2.))
                                .pl(px(2.))
                                .bg(rgb(self.hl))
                                .child(face),
                        ),
                ),
        )
    }
}

/// An inset "inventory slot" panel. Put a row or column inside.
pub fn slot(content: impl IntoElement) -> Div {
    div()
        .flex_shrink_0()
        .flex()
        .bg(rgb(SLOT_LIGHT))
        .pb(px(2.))
        .pr(px(2.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .bg(rgb(SLOT_DARK))
                .pt(px(2.))
                .pl(px(2.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .bg(rgb(SLOT_FACE))
                        .child(content),
                ),
        )
}

/// A chunky on/off switch. The caller attaches `on_click`.
pub fn switch(id: impl Into<ElementId>, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex_shrink_0()
        .cursor_pointer()
        .p(px(2.))
        .bg(rgb(BLACK))
        .child(
            div()
                .w(px(40.))
                .h(px(20.))
                .px(px(2.))
                .flex()
                .items_center()
                .when(on, |d| d.justify_end())
                .bg(rgb(if on { 0x4c9a33 } else { 0x3a3a40 }))
                .child(
                    div()
                        .size(px(16.))
                        .bg(rgb(if on { 0xe8f7dc } else { 0xb8b8c0 }))
                        .border_b_2()
                        .border_r_2()
                        .border_color(rgb(if on { 0x8fbf74 } else { 0x6a6a72 })),
                ),
        )
}

/// Small rectangular tag.
pub fn tag(text: impl Into<SharedString>, fg: u32, bg: u32) -> Div {
    div()
        .flex_shrink_0()
        .px(px(5.))
        .py(px(1.))
        .bg(rgb(bg))
        .text_size(px(10.))
        .text_color(rgb(fg))
        .child(text.into())
}

// ------------------------------------------------------------- pixel icons

pub enum Icon {
    Logo,
    Mods,
    Packs,
    Shaders,
    Download,
    Options,
}

/// Draw a pixel icon. Each character is one pixel; `.` is transparent.
pub fn icon(which: Icon, pixel: f32) -> impl IntoElement {
    let rows: &[&str] = match which {
        Icon::Logo => &[
            "kkkkkkkkk",
            "kmmmmmmmk",
            "kmdddddmk",
            "kmmmmmmmk",
            ".kmmmmmk.",
            "..kmmmk..",
            "...kmk...",
            "...kmk...",
            "....k....",
        ],
        Icon::Mods => &[
            "...k.k...",
            ".k.kkk.k.",
            "..kgggk..",
            "kkgg.ggkk",
            ".kg...gk.",
            "kkgg.ggkk",
            "..kgggk..",
            ".k.kkk.k.",
            "...k.k...",
        ],
        Icon::Packs => &[
            "bbbbbbbbb",
            "bsssssssb",
            "bsssssysb",
            "bsssssssb",
            "bsgsssssb",
            "bgggsgssb",
            "bggggggsb",
            "bgggggggb",
            "bbbbbbbbb",
        ],
        Icon::Shaders => &[
            "....y....",
            ".y..y..y.",
            "..yyyyy..",
            "..yoooy..",
            "yyyoooyyy",
            "..yoooy..",
            "..yyyyy..",
            ".y..y..y.",
            "....y....",
        ],
        Icon::Download => &[
            "...ggg...",
            "...ggg...",
            "...ggg...",
            ".ggggggg.",
            "..ggggg..",
            "...ggg...",
            "....g....",
            "m.......m",
            "mmmmmmmmm",
        ],
        Icon::Options => &[
            "..w......",
            "mmwmmmmmm",
            "..w......",
            ".........",
            "......w..",
            "mmmmmmwmm",
            "......w..",
            ".........",
            ".........",
        ],
    };
    let color = |c: char| -> Option<u32> {
        Some(match c {
            'k' => 0x1a1a1c,
            'm' => 0x8c8c94,
            'd' => 0x4a4a52,
            'g' => GREEN,
            'b' => 0x7a5232,
            's' => 0x7ab6e8,
            'y' => GOLD,
            'o' => 0xff9f2e,
            'w' => 0xf0f0f0,
            _ => return None,
        })
    };
    let mut col = div().flex_shrink_0().flex().flex_col();
    for row in rows {
        let mut line = div().flex();
        for ch in row.chars() {
            let cell = div().size(px(pixel));
            line = line.child(match color(ch) {
                Some(c) => cell.bg(rgb(c)),
                None => cell,
            });
        }
        col = col.child(line);
    }
    col
}
