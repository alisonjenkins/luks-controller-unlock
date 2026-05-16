//! tiny-skia drawing routines.
//!
//! The renderer composes the unlock UI to an offscreen RGBA `Pixmap` and
//! then copies into the DRM dumb buffer with an in-place R/B swap (DRM's
//! `XRGB8888` is byte-order B,G,R,X; tiny-skia produces R,G,B,A).
//!
//! Surface: dark background, centered rounded card, a row of dots
//! representing the entered PIN length (no symbol leak), a status pip in
//! the corner indicating controller connection. Text is drawn with a
//! tiny embedded 5x7 ASCII bitmap font so the renderer has zero external
//! asset requirements at boot — a TTF-backed font can replace this later
//! without changing the call sites.

use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect, Transform};

use crate::error::{Error, Result};
use crate::ui::drm::Frame;

fn bg() -> Color {
    Color::from_rgba8(0x12, 0x16, 0x22, 0xFF)
}
fn card() -> Color {
    Color::from_rgba8(0xF2, 0xF2, 0xF6, 0xFF)
}
fn card_shadow() -> Color {
    Color::from_rgba8(0x00, 0x00, 0x00, 0x40)
}
fn dot_on() -> Color {
    Color::from_rgba8(0x4C, 0x6E, 0xF5, 0xFF)
}
fn dot_off() -> Color {
    Color::from_rgba8(0xC8, 0xCC, 0xD4, 0xFF)
}
fn text_color() -> Color {
    Color::from_rgba8(0x10, 0x12, 0x18, 0xFF)
}
fn status_ok() -> Color {
    Color::from_rgba8(0x52, 0xC4, 0x1A, 0xFF)
}
fn status_bad() -> Color {
    Color::from_rgba8(0xE0, 0x4B, 0x4B, 0xFF)
}

#[derive(Debug, Clone)]
pub struct UiState {
    pub title: String,
    pub footer: String,
    pub pin_len: usize,
    pub controller_ok: bool,
    pub message: Option<Message>,
}

#[derive(Debug, Clone, Copy)]
pub enum Message {
    EnterPin,
    #[allow(dead_code)]
    Confirm,
    WrongPin,
    Verifying,
    PluginController,
}

impl UiState {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            footer: String::from("A=PRESS   B(HOLD)=ERASE   START=SUBMIT"),
            pin_len: 0,
            controller_ok: false,
            message: Some(Message::EnterPin),
        }
    }
}

pub fn render(frame: &mut Frame<'_>, state: &UiState) -> Result<()> {
    let w = frame.width();
    let h = frame.height();
    let mut pix = Pixmap::new(w, h)
        .ok_or_else(|| Error::Drm(format!("Pixmap::new({w}, {h}) failed")))?;
    pix.fill(bg());

    #[allow(clippy::cast_precision_loss)]
    let (wf, hf) = (w as f32, h as f32);
    let card_w = (wf * 0.58).clamp(480.0, 1100.0);
    let card_h = (hf * 0.40).clamp(280.0, 540.0);
    let card_x = (wf - card_w) * 0.5;
    let card_y = (hf - card_h) * 0.5;

    fill_rounded(&mut pix, card_x + 4.0, card_y + 8.0, card_w, card_h, 28.0, card_shadow());
    fill_rounded(&mut pix, card_x, card_y, card_w, card_h, 28.0, card());

    draw_text(
        &mut pix,
        &state.title.to_ascii_uppercase(),
        card_x + card_w * 0.5,
        card_y + 60.0,
        4,
        text_color(),
        Align::Center,
    );

    draw_dot_row(&mut pix, card_x + card_w * 0.5, card_y + card_h * 0.50, state.pin_len);

    if let Some(msg) = state.message {
        let label = match msg {
            Message::EnterPin => "ENTER PIN",
            Message::Confirm => "CONFIRM PIN",
            Message::WrongPin => "WRONG PIN",
            Message::Verifying => "VERIFYING",
            Message::PluginController => "PLUG IN A CONTROLLER",
        };
        draw_text(
            &mut pix,
            label,
            card_x + card_w * 0.5,
            card_y + card_h - 80.0,
            3,
            text_color(),
            Align::Center,
        );
    }

    draw_text(
        &mut pix,
        &state.footer,
        card_x + card_w * 0.5,
        card_y + card_h - 36.0,
        2,
        text_color(),
        Align::Center,
    );

    let pip_color = if state.controller_ok {
        status_ok()
    } else {
        status_bad()
    };
    fill_circle(&mut pix, wf - 28.0, hf - 28.0, 8.0, pip_color);

    blit_rgba_to_xrgb(&pix, frame);
    Ok(())
}

#[allow(clippy::suboptimal_flops)]
fn draw_dot_row(pix: &mut Pixmap, cx: f32, cy: f32, pin_len: usize) {
    const VISIBLE_MAX: usize = 16;
    let radius = 11.0_f32;
    let gap = 18.0_f32;
    let dots = pin_len.min(VISIBLE_MAX);
    #[allow(clippy::cast_precision_loss)]
    let row_w = (VISIBLE_MAX as f32) * (radius * 2.0 + gap) - gap;
    let row_x = cx - row_w * 0.5;
    for i in 0..VISIBLE_MAX {
        #[allow(clippy::cast_precision_loss)]
        let x = row_x + (i as f32) * (radius * 2.0 + gap) + radius;
        let on = i < dots;
        fill_circle(pix, x, cy, radius, if on { dot_on() } else { dot_off() });
    }
    if pin_len > VISIBLE_MAX {
        let extra = format!("+{}", pin_len - VISIBLE_MAX);
        draw_text(pix, &extra, cx, cy + 30.0, 2, text_color(), Align::Center);
    }
}

#[allow(clippy::many_single_char_names)]
fn fill_rounded(pix: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, color: Color) {
    let r = r.min(w * 0.5).min(h * 0.5).max(0.0);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    let Some(path) = pb.finish() else { return };
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    pix.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
}

fn fill_circle(pix: &mut Pixmap, cx: f32, cy: f32, radius: f32, color: Color) {
    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy, radius);
    let Some(path) = pb.finish() else { return };
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    pix.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
}

#[allow(dead_code)]
fn fill_rect(pix: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, color: Color) {
    let Some(rect) = Rect::from_xywh(x, y, w, h) else { return };
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    pix.fill_rect(rect, &paint, Transform::identity(), None);
}

#[derive(Debug, Clone, Copy)]
enum Align {
    Center,
    #[allow(dead_code)]
    Left,
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::suboptimal_flops,
)]
fn draw_text(pix: &mut Pixmap, text: &str, x: f32, y: f32, scale: u32, color: Color, align: Align) {
    let scale = scale.max(1);
    let glyph_w = font5x7::WIDTH * scale as usize;
    let glyph_h = font5x7::HEIGHT * scale as usize;
    let advance = glyph_w + scale as usize;
    let n = text.chars().count();
    if n == 0 {
        return;
    }
    let total_w = n * advance - scale as usize;
    let start_x = match align {
        Align::Left => x as i32,
        Align::Center => (x - (total_w as f32) * 0.5) as i32,
    };
    let baseline_y = (y - (glyph_h as f32) * 0.5) as i32;

    for (i, ch) in text.chars().enumerate() {
        let cx = start_x + (i * advance) as i32;
        font5x7::blit_glyph(pix, cx, baseline_y, ch, scale as i32, color);
    }
}

fn blit_rgba_to_xrgb(pix: &Pixmap, frame: &mut Frame<'_>) {
    let src = pix.pixels();
    // Logical (renderer) dimensions.
    let logical_w = frame.width() as usize;
    let logical_h = frame.height() as usize;
    // Native (panel) dimensions — what the dumb buffer actually is.
    let native_w = frame.native_width() as usize;
    // native_h not needed directly; stride×row indexing already
    // sized by the dumb buffer's actual byte length.
    let _native_h = frame.native_height() as usize;
    let stride = frame.stride() as usize;
    let rotation = frame.rotation();
    let dst = frame.pixels_mut();

    for ly in 0..logical_h {
        for lx in 0..logical_w {
            let p = src[ly * logical_w + lx];
            // Map logical (lx, ly) to native (px, py) per rotation.
            // For Cw90: panel's "right side up" means content rotated
            // 90° clockwise — logical top-left ends up at panel
            // top-right.
            let (px, py) = match rotation {
                crate::ui::drm::Rotation::None => (lx, ly),
                crate::ui::drm::Rotation::Cw90 => (native_w - 1 - ly, lx),
            };
            let off = py * stride + px * 4;
            dst[off] = p.blue();
            dst[off + 1] = p.green();
            dst[off + 2] = p.red();
            dst[off + 3] = 0xFF;
        }
    }
}

mod font5x7 {
    //! Minimal 5x7 ASCII bitmap font. Covers digits, A–Z, space, and a
    //! few punctuation marks needed for the UI footer. Each glyph is
    //! 5 columns by 7 rows packed into 5 bytes (one per column,
    //! low bit = top row).

    use tiny_skia::{Color, Pixmap};

    pub const WIDTH: usize = 5;
    pub const HEIGHT: usize = 7;

    #[allow(
        clippy::cast_possible_wrap,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
    )]
    pub fn blit_glyph(pix: &mut Pixmap, x: i32, y: i32, ch: char, scale: i32, color: Color) {
        let glyph = lookup(ch);
        let pixmap_w = pix.width() as i32;
        let pixmap_h = pix.height() as i32;
        let pre = color.premultiply().to_color_u8();
        let raw = pix.pixels_mut();
        for col in 0..WIDTH as i32 {
            let bits = glyph[col as usize];
            for row in 0..HEIGHT as i32 {
                if bits & (1 << row) == 0 {
                    continue;
                }
                for sx in 0..scale {
                    for sy in 0..scale {
                        let px = x + col * scale + sx;
                        let py = y + row * scale + sy;
                        if px < 0 || py < 0 || px >= pixmap_w || py >= pixmap_h {
                            continue;
                        }
                        let idx = (py * pixmap_w + px) as usize;
                        raw[idx] = pre;
                    }
                }
            }
        }
    }

    const fn lookup(ch: char) -> [u8; 5] {
        let upper = ch.to_ascii_uppercase();
        match upper {
            ' ' => [0; 5],
            '.' => [0x00, 0x00, 0x60, 0x60, 0x00],
            ',' => [0x00, 0x80, 0x60, 0x60, 0x00],
            ':' => [0x00, 0x36, 0x36, 0x00, 0x00],
            '-' => [0x08, 0x08, 0x08, 0x08, 0x08],
            '+' => [0x08, 0x08, 0x3E, 0x08, 0x08],
            '=' => [0x14, 0x14, 0x14, 0x14, 0x14],
            '(' => [0x00, 0x1C, 0x22, 0x41, 0x00],
            ')' => [0x00, 0x41, 0x22, 0x1C, 0x00],
            '/' => [0x40, 0x20, 0x10, 0x08, 0x04],
            '0' => [0x3E, 0x51, 0x49, 0x45, 0x3E],
            '1' => [0x00, 0x42, 0x7F, 0x40, 0x00],
            '2' => [0x42, 0x61, 0x51, 0x49, 0x46],
            '3' => [0x21, 0x41, 0x45, 0x4B, 0x31],
            '4' => [0x18, 0x14, 0x12, 0x7F, 0x10],
            '5' => [0x27, 0x45, 0x45, 0x45, 0x39],
            '6' => [0x3C, 0x4A, 0x49, 0x49, 0x30],
            '7' => [0x01, 0x71, 0x09, 0x05, 0x03],
            '8' => [0x36, 0x49, 0x49, 0x49, 0x36],
            '9' => [0x06, 0x49, 0x49, 0x29, 0x1E],
            'A' => [0x7E, 0x11, 0x11, 0x11, 0x7E],
            'B' => [0x7F, 0x49, 0x49, 0x49, 0x36],
            'C' => [0x3E, 0x41, 0x41, 0x41, 0x22],
            'D' => [0x7F, 0x41, 0x41, 0x22, 0x1C],
            'E' => [0x7F, 0x49, 0x49, 0x49, 0x41],
            'F' => [0x7F, 0x09, 0x09, 0x09, 0x01],
            'G' => [0x3E, 0x41, 0x49, 0x49, 0x7A],
            'H' => [0x7F, 0x08, 0x08, 0x08, 0x7F],
            'I' => [0x00, 0x41, 0x7F, 0x41, 0x00],
            'J' => [0x20, 0x40, 0x41, 0x3F, 0x01],
            'K' => [0x7F, 0x08, 0x14, 0x22, 0x41],
            'L' => [0x7F, 0x40, 0x40, 0x40, 0x40],
            'M' => [0x7F, 0x02, 0x0C, 0x02, 0x7F],
            'N' => [0x7F, 0x04, 0x08, 0x10, 0x7F],
            'O' => [0x3E, 0x41, 0x41, 0x41, 0x3E],
            'P' => [0x7F, 0x09, 0x09, 0x09, 0x06],
            'Q' => [0x3E, 0x41, 0x51, 0x21, 0x5E],
            'R' => [0x7F, 0x09, 0x19, 0x29, 0x46],
            'S' => [0x46, 0x49, 0x49, 0x49, 0x31],
            'T' => [0x01, 0x01, 0x7F, 0x01, 0x01],
            'U' => [0x3F, 0x40, 0x40, 0x40, 0x3F],
            'V' => [0x1F, 0x20, 0x40, 0x20, 0x1F],
            'W' => [0x7F, 0x20, 0x18, 0x20, 0x7F],
            'X' => [0x63, 0x14, 0x08, 0x14, 0x63],
            'Y' => [0x03, 0x04, 0x78, 0x04, 0x03],
            'Z' => [0x61, 0x51, 0x49, 0x45, 0x43],
            _ => [0x7F, 0x41, 0x41, 0x41, 0x7F], // hollow box for unknown
        }
    }
}
