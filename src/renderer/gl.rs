//! gl.rs — OpenGL ES 3 renderer.
//!
//! Consumes a `Vec<DrawCmd>` and flushes it to the currently-bound FBO via
//! four instanced draw passes:
//!
//!   1. BG quads      — FillRect, StrokeRect, HLine, VLine, BorderLine,
//!                      box-drawing geometry (from Text), shade blocks
//!   2. Round-rect    — RoundRect (SDF, separate program)
//!   3. Glyph quads   — Text (HarfBuzz shaped), braille atlas
//!   4. Tri pass      — PowerlineArrow + Powerline chars inside Text strings
//!
//! NDC conversion — THE ONLY PLACE it lives:
//!   ndc.x = (px.x / vp.w) * 2.0 - 1.0
//!   ndc.y = (px.y / vp.h) * 2.0 - 1.0   ← no Y-flip; DRM FBO is top-left origin
//!
//! ── BorderLine ────────────────────────────────────────────────────────────────
//! DrawCmd::BorderLine is decomposed into plain BgInst rects here — one per
//! enabled side. Widgets should emit BorderLine instead of box-draw characters.
//!
//! ── Box-drawing (TUI compat) ──────────────────────────────────────────────────
//! U+2500–U+257F and U+2580–U+259F embedded inside DrawCmd::Text strings are
//! still decoded to BgInst geometry for backward compatibility with raw TUI
//! output. New widget code should NOT rely on this path.
//!
//! ── RoundRect (SDF) ──────────────────────────────────────────────────────────
//! Each instance carries (x,y,w,h), per-corner radii (vec4), fill colour,
//! stroke colour, and stroke width. The fragment shader evaluates a box-SDF
//! and discards fragments outside the rounded shape, giving sub-pixel-quality
//! anti-aliased edges with no multisampling required.
//!
//! ── Shade blocks ─────────────────────────────────────────────────────────────
//! ░▒▓ (U+2591–U+2593) inside Text are rendered as full-cell BgInst quads with
//! premultiplied alpha. Requires premultiplied blend (ONE / ONE_MINUS_SRC_ALPHA).
//!
//! ── PowerlineArrow + Powerline chars ─────────────────────────────────────────
//! DrawCmd::PowerlineArrow emits directly to TriInst.
//! U+E0B0–U+E0B3 inside Text strings are also converted to TriInst (TUI compat).
use std::collections::HashMap;
use std::ffi::CString;
use std::mem::size_of;
use std::pin::Pin;

use ab_glyph::{Font, FontRef, GlyphId, PxScale, ScaleFont};
use rustybuzz::{Face, UnicodeBuffer};

use crate::renderer::{BorderSide, Color, CornerRadius, DrawCmd, PowerlineDir, TextStyle};

// ═════════════════════════════════════════════════════════════════════════════
// Shaders
// ═════════════════════════════════════════════════════════════════════════════

const BG_VERT: &str = r#"#version 300 es
precision highp float;
in vec2 a_pos;
in vec4 i_rect;
in vec4 i_color;
uniform vec2 u_vp;
out vec4 v_color;
void main() {
    vec2 px  = i_rect.xy + a_pos * i_rect.zw;
    vec2 ndc = vec2((px.x / u_vp.x) * 2.0 - 1.0, (px.y / u_vp.y) * 2.0 - 1.0);
    gl_Position = vec4(ndc, 0.0, 1.0);
    v_color = i_color;
}
"#;

const BG_FRAG: &str = r#"#version 300 es
precision mediump float;
in vec4 v_color;
out vec4 fragColor;
void main() { fragColor = v_color; }
"#;

// ── RoundRect SDF ────────────────────────────────────────────────────────────
//
// Instance layout (96 bytes):
//   vec4 i_rect     — (x, y, w, h) in pixels
//   vec4 i_radii    — (tl, tr, bl, br) corner radii
//   vec4 i_fill     — fill RGBA premultiplied
//   vec4 i_stroke   — stroke RGBA premultiplied
//   float i_strokew — stroke width in pixels
//   float[3] _pad
//
// The vertex shader emits a full-rect quad in NDC. The fragment shader
// computes the box-SDF in local (uv) space and discards/blends accordingly.

const RRECT_VERT: &str = r#"#version 300 es
precision highp float;

// Per-vertex (quad template 0..5)
in vec2 a_pos;

// Per-instance
in vec4 i_rect;
in vec4 i_radii;
in vec4 i_fill;
in vec4 i_stroke;
in float i_strokew;

uniform vec2 u_vp;

out vec2  v_uv;     // 0..1 across the rect
out vec2  v_size;   // (w, h) in pixels — for SDF in pixel space
out vec4  v_radii;
out vec4  v_fill;
out vec4  v_stroke;
out float v_strokew;

void main() {
    vec2 px  = i_rect.xy + a_pos * i_rect.zw;
    vec2 ndc = vec2((px.x / u_vp.x) * 2.0 - 1.0, (px.y / u_vp.y) * 2.0 - 1.0);
    gl_Position = vec4(ndc, 0.0, 1.0);
    v_uv     = a_pos;
    v_size   = i_rect.zw;
    v_radii  = i_radii;
    v_fill   = i_fill;
    v_stroke = i_stroke;
    v_strokew = i_strokew;
}
"#;

const RRECT_FRAG: &str = r#"#version 300 es
precision highp float;

in vec2  v_uv;
in vec2  v_size;
in vec4  v_radii;
in vec4  v_fill;
in vec4  v_stroke;
in float v_strokew;

out vec4 fragColor;

// Box SDF with per-corner radii.
// p  — point in [-half_size .. +half_size] space
// b  — half extents (w/2, h/2)
// r  — radius for this quadrant
float sdf_round_box(vec2 p, vec2 b, float r) {
    vec2 q = abs(p) - b + r;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

void main() {
    vec2 half_size = v_size * 0.5;
    // Point in centred pixel space
    vec2 p = (v_uv - 0.5) * v_size;

    // Pick radius for this quadrant
    // v_radii = (tl, tr, bl, br)
    float r;
    if (p.x < 0.0 && p.y < 0.0) r = v_radii.x;
    else if (p.x >= 0.0 && p.y < 0.0) r = v_radii.y;
    else if (p.x < 0.0) r = v_radii.z;
    else r = v_radii.w;

    float d = sdf_round_box(p, half_size, r);

    // Anti-alias width (1 px)
    float aa = fwidth(d);

    // Outer edge alpha
    float outer = 1.0 - smoothstep(-aa, aa, d);
    if (outer < 0.001) discard;

    vec4 col = vec4(0.0);

    bool has_fill   = v_fill.a   > 0.001;
    bool has_stroke = v_stroke.a > 0.001 && v_strokew > 0.0;

    if (has_fill) {
        col = v_fill;
    }

    if (has_stroke) {
        float inner_d = d + v_strokew;
        float stroke_mask = smoothstep(-aa, aa, inner_d)
                          * (1.0 - smoothstep(-aa, aa, d));
        col = mix(col, v_stroke, stroke_mask);
    }

    col *= outer;
    if (col.a < 0.001) discard;
    fragColor = col;
}
"#;

const GLYPH_VERT: &str = r#"#version 300 es
precision highp float;
in vec2 a_pos;
in vec4 i_glyph;
in vec4 i_uv;
in vec4 i_fg;
uniform vec2 u_vp;
out vec2 v_uv;
out vec4 v_fg;
void main() {
    vec2 px  = i_glyph.xy + a_pos * i_glyph.zw;
    vec2 ndc = vec2((px.x / u_vp.x) * 2.0 - 1.0, (px.y / u_vp.y) * 2.0 - 1.0);
    gl_Position = vec4(ndc, 0.0, 1.0);
    v_uv = mix(i_uv.xy, i_uv.zw, a_pos);
    v_fg = i_fg;
}
"#;

const GLYPH_FRAG: &str = r#"#version 300 es
precision mediump float;
uniform sampler2D u_atlas;
in vec2 v_uv;
in vec4 v_fg;
out vec4 fragColor;
void main() {
    float a = texture(u_atlas, v_uv).a;
    if (a < 0.004) discard;
    a = pow(a, 0.5);
    float alpha = v_fg.a * a;
    fragColor = vec4(v_fg.rgb * alpha, alpha);
}
"#;

const POWERLINE_VERT: &str = r#"#version 300 es
precision highp float;
in vec4 i_rect;
in vec4 i_color;
in float i_dir;
uniform vec2 u_vp;
out vec4 v_color;

void main() {
    float x = i_rect.x, y = i_rect.y, w = i_rect.z, h = i_rect.w;
    vec2 pos;

    if (i_dir < 2.0) {
        vec2 pts[3];
        if (i_dir < 0.5) {
            pts[0] = vec2(x,     y);
            pts[1] = vec2(x,     y + h);
            pts[2] = vec2(x + w, y + h * 0.5);
        } else {
            pts[0] = vec2(x + w, y);
            pts[1] = vec2(x + w, y + h);
            pts[2] = vec2(x,     y + h * 0.5);
        }
        pos = pts[gl_VertexID];
    } else {
        float bx = (i_dir < 2.5) ? x       : x + w;
        float tx = (i_dir < 2.5) ? x + w   : x;
        vec2 A  = vec2(bx, y);
        vec2 C  = vec2(bx, y + h);
        vec2 Bp = vec2(tx, y + h * 0.5);

        const float AW = 1.5;
        vec2 d1 = normalize(Bp - A);
        vec2 p1 = vec2(-d1.y, d1.x) * AW;
        vec2 d2 = normalize(Bp - C);
        vec2 p2 = vec2(-d2.y, d2.x) * AW;

        vec2 ta[6];
        ta[0] = A  - p1;  ta[1] = A  + p1;  ta[2] = Bp - p1;
        ta[3] = A  + p1;  ta[4] = Bp + p1;  ta[5] = Bp - p1;

        vec2 ba[6];
        ba[0] = C  + p2;  ba[1] = C  - p2;  ba[2] = Bp + p2;
        ba[3] = C  - p2;  ba[4] = Bp - p2;  ba[5] = Bp + p2;

        if (gl_VertexID < 6) pos = ta[gl_VertexID];
        else                  pos = ba[gl_VertexID - 6];
    }

    vec2 ndc = vec2((pos.x / u_vp.x) * 2.0 - 1.0, (pos.y / u_vp.y) * 2.0 - 1.0);
    gl_Position = vec4(ndc, 0.0, 1.0);
    v_color = i_color;
}
"#;

const POWERLINE_FRAG: &str = r#"#version 300 es
precision mediump float;
in vec4 v_color;
out vec4 fragColor;
void main() { fragColor = v_color; }
"#;

// ═════════════════════════════════════════════════════════════════════════════
// Instance structs (GPU layout)
// ═════════════════════════════════════════════════════════════════════════════

#[repr(C)]
#[derive(Clone, Copy)]
struct BgInst {
    rect: [f32; 4],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RRectInst {
    rect: [f32; 4],   // x, y, w, h
    radii: [f32; 4],  // tl, tr, bl, br
    fill: [f32; 4],   // premultiplied RGBA
    stroke: [f32; 4], // premultiplied RGBA
    strokew: f32,
    _pad: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct GlyphInst {
    glyph: [f32; 4],
    uv: [f32; 4],
    fg: [f32; 4],
}

/// Powerline triangle instance (filled arrows: 3 verts; chevrons: 12 verts).
#[repr(C)]
#[derive(Clone, Copy)]
struct TriInst {
    rect: [f32; 4],
    color: [f32; 4],
    dir: f32,
    _pad: [f32; 3],
}

#[rustfmt::skip]
const QUAD: [f32; 12] = [
    0.0, 0.0,  1.0, 0.0,  1.0, 1.0,
    0.0, 0.0,  1.0, 1.0,  0.0, 1.0,
];

// ═════════════════════════════════════════════════════════════════════════════
// GlyphAtlas
// ═════════════════════════════════════════════════════════════════════════════

pub const ATLAS_DIM: u32 = 2048;
const ATLAS_GAP: u32 = 1;

#[derive(Clone, Copy, Debug)]
pub struct GlyphUv {
    pub uv_x: f32,
    pub uv_y: f32,
    pub uv_w: f32,
    pub uv_h: f32,
    pub width: u32,
    pub height: u32,
    pub bearing_x: i32,
    pub bearing_y: i32,
    pub advance: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CharKey {
    ch: char,
    bold: bool,
    italic: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct IdKey {
    id: u16,
    bold: bool,
    italic: bool,
}

struct OwnedFace {
    _bytes: Pin<Box<[u8]>>,
    font: FontRef<'static>,
    scale: PxScale,
}

impl OwnedFace {
    fn new(data: Vec<u8>, scale: PxScale) -> Result<Self, String> {
        let pinned: Pin<Box<[u8]>> = Pin::new(data.into_boxed_slice());

        let bytes_static: &'static [u8] = unsafe {
            let ptr = pinned.as_ptr();
            let len = pinned.len();
            std::slice::from_raw_parts(ptr, len)
        };

        let font = FontRef::try_from_slice(bytes_static).map_err(|e| format!("ab_glyph: {e}"))?;

        Ok(Self {
            _bytes: pinned,
            font,
            scale,
        })
    }

    #[inline]
    fn scaled(&self) -> ab_glyph::PxScaleFont<&FontRef<'static>> {
        self.font.as_scaled(self.scale)
    }
}

/// 2048×2048 RGBA8 glyph atlas.
///
/// Box-drawing (U+2500–U+257F), block elements (U+2580–U+259F), and Powerline
/// arrows (U+E0B0–U+E0B3) are **not** loaded here — they are rendered as native
/// GL geometry. Braille (U+2800–U+28FF) still uses the atlas.
pub struct GlyphAtlas {
    regular: OwnedFace,
    bold: Option<OwnedFace>,
    italic: Option<OwnedFace>,

    char_cache: HashMap<CharKey, Option<GlyphUv>>,
    id_cache: HashMap<IdKey, Option<GlyphUv>>,

    pub pixels: Vec<u8>,
    pub cursor_x: u32,
    pub cursor_y: u32,
    pub row_h: u32,
    pub cell_w: u32,
    pub cell_h: u32,
    pub natural_h: u32,
    pub ascender: i32,
    pub dirty: bool,
}

impl GlyphAtlas {
    pub fn new(
        regular_data: &[u8],
        bold_data: Option<&[u8]>,
        italic_data: Option<&[u8]>,
        size_px: f32,
        _line_spacing: f32,
    ) -> Result<Self, String> {
        let scale = PxScale::from(size_px);
        let regular = OwnedFace::new(regular_data.to_vec(), scale)?;
        let bold = bold_data
            .map(|d| OwnedFace::new(d.to_vec(), scale))
            .transpose()
            .unwrap_or(None);
        let italic = italic_data
            .map(|d| OwnedFace::new(d.to_vec(), scale))
            .transpose()
            .unwrap_or(None);

        let sf = regular.scaled();
        let natural_h = (sf.ascent() + sf.descent().abs() + sf.line_gap()).ceil() as u32;
        let cell_h = (natural_h + 3).max(1);
        let ascender = sf.ascent().ceil() as i32;
        let cell_w = {
            let id = sf.font.glyph_id('0');
            let adv = sf.h_advance(id).ceil() as u32;
            if adv > 0 {
                adv
            } else {
                let sid = sf.font.glyph_id(' ');
                (sf.h_advance(sid).ceil() as u32).max(size_px as u32)
            }
        }
        .max(4);

        tracing::info!(
            "GlyphAtlas {size_px:.1}px cell={cell_w}×{cell_h} natural={natural_h} asc={ascender}"
        );

        let mut atlas = Self {
            regular,
            bold,
            italic,
            char_cache: HashMap::new(),
            id_cache: HashMap::new(),
            pixels: vec![0u8; (ATLAS_DIM * ATLAS_DIM * 4) as usize],
            cursor_x: 0,
            cursor_y: 0,
            row_h: 0,
            natural_h,
            cell_w,
            cell_h,
            ascender,
            dirty: true,
        };

        for ch in ' '..='~' {
            atlas.glyph(ch, false, false);
            atlas.glyph(ch, true, false);
            atlas.glyph(ch, false, true);
        }
        for cp in 0x2800u32..=0x28FF {
            if let Some(ch) = char::from_u32(cp) {
                atlas.glyph(ch, false, false);
            }
        }
        Ok(atlas)
    }

    pub fn glyph(&mut self, ch: char, bold: bool, italic: bool) -> Option<GlyphUv> {
        let key = CharKey { ch, bold, italic };
        if let Some(&v) = self.char_cache.get(&key) {
            return v;
        }
        let info = self.rasterise_char(ch, bold, italic);
        self.char_cache.insert(key, info);
        info
    }

    pub fn glyph_by_id(&mut self, id: u16, bold: bool, italic: bool) -> Option<GlyphUv> {
        let key = IdKey { id, bold, italic };
        if let Some(&v) = self.id_cache.get(&key) {
            return v;
        }
        let info = self.rasterise_by_id(id, bold, italic);
        self.id_cache.insert(key, info);
        info
    }

    fn rasterise_char(&mut self, ch: char, bold: bool, italic: bool) -> Option<GlyphUv> {
        let fp = self.pick_face(bold, italic);
        let id = unsafe { (*fp).scaled().font.glyph_id(ch) };
        let fp = if id == GlyphId(0) && (bold || italic) {
            &self.regular as *const _
        } else {
            fp
        };
        let id = unsafe { (*fp).scaled().font.glyph_id(ch) };
        self.rasterise_from_ptr(id, fp)
    }

    fn rasterise_by_id(&mut self, id: u16, bold: bool, italic: bool) -> Option<GlyphUv> {
        let fp = self.pick_face(bold, italic);
        self.rasterise_from_ptr(GlyphId(id), fp)
    }

    fn pick_face(&self, bold: bool, italic: bool) -> *const OwnedFace {
        if bold && self.bold.is_some() {
            self.bold.as_ref().unwrap()
        } else if italic && self.italic.is_some() {
            self.italic.as_ref().unwrap()
        } else {
            &self.regular
        }
    }

    fn rasterise_from_ptr(&mut self, glyph_id: GlyphId, fp: *const OwnedFace) -> Option<GlyphUv> {
        let sf = unsafe { (*fp).scaled() };
        let advance = sf.h_advance(glyph_id).ceil() as u32;
        let glyph = glyph_id.with_scale_and_position(sf.scale, ab_glyph::point(0.0, sf.ascent()));
        let outlined = sf.font.outline_glyph(glyph);
        drop(sf);

        let Some(outlined) = outlined else {
            return Some(GlyphUv {
                uv_x: 0.,
                uv_y: 0.,
                uv_w: 0.,
                uv_h: 0.,
                width: 0,
                height: 0,
                bearing_x: 0,
                bearing_y: 0,
                advance,
            });
        };

        let bounds = outlined.px_bounds();
        let w = bounds.width().ceil() as u32;
        let h = bounds.height().ceil() as u32;
        let bearing_x = bounds.min.x.floor() as i32;
        let bearing_y = (-bounds.min.y).ceil() as i32;

        if w == 0 || h == 0 {
            return Some(GlyphUv {
                uv_x: 0.,
                uv_y: 0.,
                uv_w: 0.,
                uv_h: 0.,
                width: 0,
                height: 0,
                bearing_x,
                bearing_y,
                advance,
            });
        }

        let mut cov = vec![0u8; (w * h) as usize];
        outlined.draw(|px, py, c| {
            let i = (py * w + px) as usize;
            if i < cov.len() {
                cov[i] = (c * 255.0).round() as u8;
            }
        });
        self.blit(cov, w, h, bearing_x, bearing_y, advance)
    }

    fn blit(
        &mut self,
        bitmap: Vec<u8>,
        w: u32,
        h: u32,
        bearing_x: i32,
        bearing_y: i32,
        advance: u32,
    ) -> Option<GlyphUv> {
        let h = h.min(self.cell_h * 2);
        if self.cursor_x + w + ATLAS_GAP > ATLAS_DIM {
            self.cursor_y += self.row_h + ATLAS_GAP;
            self.cursor_x = 0;
            self.row_h = 0;
        }
        if self.cursor_y + h + ATLAS_GAP > ATLAS_DIM {
            tracing::error!("GlyphAtlas: atlas full — glyph dropped");
            return None;
        }
        let stride = ATLAS_DIM as usize;
        for py in 0..h {
            for px in 0..w {
                let src = bitmap[(py * w + px) as usize];
                let base =
                    ((self.cursor_y + py) as usize * stride + (self.cursor_x + px) as usize) * 4;
                self.pixels[base] = 0xFF;
                self.pixels[base + 1] = 0xFF;
                self.pixels[base + 2] = 0xFF;
                self.pixels[base + 3] = src;
            }
        }
        let inset = 0.5 / ATLAS_DIM as f32;
        let uv = GlyphUv {
            uv_x: self.cursor_x as f32 / ATLAS_DIM as f32 + inset,
            uv_y: self.cursor_y as f32 / ATLAS_DIM as f32 + inset,
            uv_w: w as f32 / ATLAS_DIM as f32 - inset * 2.0,
            uv_h: h as f32 / ATLAS_DIM as f32 - inset * 2.0,
            width: w,
            height: h,
            bearing_x,
            bearing_y,
            advance,
        };
        self.cursor_x += w + ATLAS_GAP;
        if h > self.row_h {
            self.row_h = h;
        }
        self.dirty = true;
        Some(uv)
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Shaper
// ═════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct ShapedGlyph {
    pub glyph_id: u16,
    pub cluster_width: usize,
    pub advance_px: f32,
}

pub struct Shaper {
    _bytes: Pin<Box<[u8]>>,
    face: Face<'static>,
}

impl Shaper {
    pub fn new(font_data: &[u8]) -> Self {
        let pinned: Pin<Box<[u8]>> = Pin::new(font_data.to_vec().into_boxed_slice());

        let face = unsafe {
            let ptr = pinned.as_ptr();
            let len = pinned.len();
            let s: &'static [u8] = std::slice::from_raw_parts(ptr, len);
            Face::from_slice(s, 0).expect("rustybuzz: bad font")
        };

        Self {
            _bytes: pinned,
            face,
        }
    }

    pub fn shape(&self, text: &str) -> Vec<ShapedGlyph> {
        if text.is_empty() {
            return vec![];
        }
        let mut buf = UnicodeBuffer::new();
        buf.push_str(text);
        let out = rustybuzz::shape(&self.face, &[], buf);
        let positions = out.glyph_positions();
        let infos = out.glyph_infos();
        (0..infos.len())
            .map(|i| {
                let cb = infos[i].cluster as usize;
                let nb = infos
                    .get(i + 1)
                    .map(|g| g.cluster as usize)
                    .unwrap_or(text.len());
                let cw = text[cb..nb].chars().count().max(1);
                ShapedGlyph {
                    glyph_id: infos[i].glyph_id as u16,
                    cluster_width: cw,
                    advance_px: positions[i].x_advance as f32,
                }
            })
            .collect()
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Codepoint classification helpers (TUI compat — Text path only)
// ═════════════════════════════════════════════════════════════════════════════

#[inline]
fn is_box_draw_cp(cp: u32) -> bool {
    matches!(cp, 0x2500..=0x257F | 0x2580..=0x259F)
}
#[inline]
fn is_shade_block(ch: char) -> bool {
    matches!(ch, '\u{2591}' | '\u{2592}' | '\u{2593}')
}
#[inline]
fn shade_alpha(ch: char) -> f32 {
    match ch {
        '\u{2591}' => 0.25,
        '\u{2592}' => 0.50,
        _ => 0.75,
    }
}
#[inline]
fn is_powerline_cp(cp: u32) -> bool {
    matches!(cp, 0xE0B0 | 0xE0B1 | 0xE0B2 | 0xE0B3)
}
#[inline]
fn powerline_dir_f32(ch: char) -> f32 {
    match ch {
        '\u{E0B0}' => 0.0,
        '\u{E0B2}' => 1.0,
        '\u{E0B1}' => 2.0,
        _ => 3.0,
    }
}
#[inline]
fn is_atlas_synthetic_cp(cp: u32) -> bool {
    matches!(cp, 0x2800..=0x28FF)
}
#[inline]
fn run_is_atlas_synthetic(text: &str) -> bool {
    text.chars()
        .all(|c| is_atlas_synthetic_cp(c as u32) || is_box_draw_cp(c as u32))
}

// ═════════════════════════════════════════════════════════════════════════════
// Box-drawing geometry (TUI compat — called only from Text path)
// ═════════════════════════════════════════════════════════════════════════════

fn box_to_lines(ch: char, cell_x: u32, cell_y: u32, cw: u32, ch_: u32) -> Vec<[f32; 4]> {
    let cx = cell_x + cw / 2;
    let cy = cell_y + ch_ / 2;
    let right = cw - cw / 2;
    let down = ch_ - ch_ / 2;
    let left1 = cw / 2 + 1;
    let up1 = ch_ / 2 + 1;

    let h = |px: u32, py: u32, w: u32| -> [f32; 4] { [px as f32, py as f32, w as f32, 1.0] };
    let v = |px: u32, py: u32, ht: u32| -> [f32; 4] { [px as f32, py as f32, 1.0, ht as f32] };
    let cp = ch as u32;

    if matches!(cp, 0x2580..=0x259F) {
        let (bx, by, bw, bh) = block_element_rect(ch, cell_x, cell_y, cw, ch_);
        return if bw > 0 && bh > 0 {
            vec![[bx as f32, by as f32, bw as f32, bh as f32]]
        } else {
            vec![]
        };
    }

    match ch {
        '─' => vec![h(cell_x, cy, cw)],
        '│' => vec![v(cx, cell_y, ch_)],
        '╭' | '┌' => vec![h(cx, cy, right), v(cx, cy, down)],
        '╰' | '└' => vec![h(cx, cy, right), v(cx, cell_y, up1)],
        '╮' | '┐' => vec![h(cell_x, cy, left1), v(cx, cy, down)],
        '╯' | '┘' => vec![h(cell_x, cy, left1), v(cx, cell_y, up1)],
        '├' => vec![v(cx, cell_y, ch_), h(cx, cy, right)],
        '┤' => vec![v(cx, cell_y, ch_), h(cell_x, cy, left1)],
        '┬' => vec![h(cell_x, cy, cw), v(cx, cy, down)],
        '┴' => vec![h(cell_x, cy, cw), v(cx, cell_y, up1)],
        '┼' => vec![h(cell_x, cy, cw), v(cx, cell_y, ch_)],
        '═' => {
            let o = (ch_ / 6).max(1);
            vec![h(cell_x, cy.saturating_sub(o), cw), h(cell_x, cy + o, cw)]
        }
        '║' => {
            let o = (cw / 6).max(1);
            vec![v(cx.saturating_sub(o), cell_y, ch_), v(cx + o, cell_y, ch_)]
        }
        '█' => vec![[cell_x as f32, cell_y as f32, cw as f32, ch_ as f32]],
        '▀' => vec![[cell_x as f32, cell_y as f32, cw as f32, (ch_ / 2) as f32]],
        '▄' => vec![[
            cell_x as f32,
            (cell_y + ch_ / 2) as f32,
            cw as f32,
            (ch_ - ch_ / 2) as f32,
        ]],
        '▌' => vec![[cell_x as f32, cell_y as f32, (cw / 2) as f32, ch_ as f32]],
        '▐' => vec![[
            (cell_x + cw / 2) as f32,
            cell_y as f32,
            (cw - cw / 2) as f32,
            ch_ as f32,
        ]],
        '╴' => vec![h(cell_x, cy, left1)],
        '╵' => vec![v(cx, cell_y, up1)],
        '╶' => vec![h(cx, cy, right)],
        '╷' => vec![v(cx, cy, down)],
        '━' => vec![h(cell_x, cy, cw), h(cell_x, cy + 1, cw)],
        '┃' => vec![v(cx, cell_y, ch_), v(cx + 1, cell_y, ch_)],
        _ => vec![],
    }
}

fn block_element_rect(
    ch: char,
    cell_x: u32,
    cell_y: u32,
    cw: u32,
    ch_: u32,
) -> (u32, u32, u32, u32) {
    match ch {
        '█' => (cell_x, cell_y, cw, ch_),
        '▀' => (cell_x, cell_y, cw, ch_ / 2),
        '▄' => (cell_x, cell_y + ch_ / 2, cw, ch_ - ch_ / 2),
        '▌' => (cell_x, cell_y, cw / 2, ch_),
        '▐' => (cell_x + cw / 2, cell_y, cw - cw / 2, ch_),
        '▘' => (cell_x, cell_y, cw / 2, ch_ / 2),
        '▝' => (cell_x + cw / 2, cell_y, cw - cw / 2, ch_ / 2),
        '▖' => (cell_x, cell_y + ch_ / 2, cw / 2, ch_ - ch_ / 2),
        '▗' => (
            cell_x + cw / 2,
            cell_y + ch_ / 2,
            cw - cw / 2,
            ch_ - ch_ / 2,
        ),
        '░' | '▒' | '▓' => (cell_x, cell_y, 0, 0),
        _ => (cell_x, cell_y, 0, 0),
    }
}

#[inline]
fn premul(c: [f32; 4]) -> [f32; 4] {
    [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]]
}

/// Convert a `Color` to a premultiplied f32 array.
#[inline]
fn color_premul(c: Color) -> [f32; 4] {
    premul(c.to_f32())
}

// ═════════════════════════════════════════════════════════════════════════════
// ChromeRenderer
// ═════════════════════════════════════════════════════════════════════════════

/// OpenGL ES 3 renderer. One instance per GL context.
pub struct ChromeRenderer {
    bg_prog: u32,
    bg_vao: u32,
    bg_ivbo: u32,
    bg_cap: usize,

    // ── RoundRect SDF pass ───────────────────────────────────────────────────
    rrect_prog: u32,
    rrect_vao: u32,
    rrect_ivbo: u32,
    rrect_cap: usize,

    glyph_prog: u32,
    glyph_vao: u32,
    glyph_ivbo: u32,
    glyph_cap: usize,

    tri_prog: u32,
    tri_fill_vao: u32,
    tri_fill_ivbo: u32,
    tri_fill_cap: usize,
    tri_out_vao: u32,
    tri_out_ivbo: u32,
    tri_out_cap: usize,

    atlas_tex: u32,
    pub atlas: GlyphAtlas,
    shaper: Shaper,
    hb_scale: f32,

    pub cell_w: u32,
    pub cell_h: u32,
    ascender: i32,
}

impl ChromeRenderer {
    pub fn new(
        atlas: GlyphAtlas,
        shaper: Shaper,
        hb_units_per_em: f32,
        size_px: f32,
    ) -> Result<Self, String> {
        let cell_w = atlas.cell_w;
        let cell_h = atlas.cell_h;
        let ascender = atlas.ascender;
        let hb_scale = size_px / hb_units_per_em;

        let bg_prog = unsafe { compile_prog(BG_VERT, BG_FRAG)? };
        let rrect_prog = unsafe { compile_prog(RRECT_VERT, RRECT_FRAG)? };
        let glyph_prog = unsafe { compile_prog(GLYPH_VERT, GLYPH_FRAG)? };
        let tri_prog = unsafe { compile_prog(POWERLINE_VERT, POWERLINE_FRAG)? };

        let (bg_vao, bg_ivbo) = unsafe { create_bg_vao(bg_prog, 1024) };
        let (rrect_vao, rrect_ivbo) = unsafe { create_rrect_vao(rrect_prog, 256) };
        let (glyph_vao, glyph_ivbo) = unsafe { create_glyph_vao(glyph_prog, 4096) };
        let (tri_fill_vao, tri_fill_ivbo) = unsafe { create_tri_vao(tri_prog, 64) };
        let (tri_out_vao, tri_out_ivbo) = unsafe { create_tri_vao(tri_prog, 64) };
        let atlas_tex = unsafe { upload_atlas_tex(&atlas) };

        Ok(Self {
            bg_prog,
            bg_vao,
            bg_ivbo,
            bg_cap: 1024,
            rrect_prog,
            rrect_vao,
            rrect_ivbo,
            rrect_cap: 256,
            glyph_prog,
            glyph_vao,
            glyph_ivbo,
            glyph_cap: 4096,
            tri_prog,
            tri_fill_vao,
            tri_fill_ivbo,
            tri_fill_cap: 64,
            tri_out_vao,
            tri_out_ivbo,
            tri_out_cap: 64,
            atlas_tex,
            atlas,
            shaper,
            hb_scale,
            cell_w,
            cell_h,
            ascender,
        })
    }

    /// Flush `cmds` into the currently-bound FBO.
    pub fn flush(&mut self, cmds: &[DrawCmd], vp_w: u32, vp_h: u32) {
        if cmds.is_empty() || vp_w == 0 || vp_h == 0 {
            return;
        }

        let mut bg_cpu: Vec<BgInst> = Vec::with_capacity(cmds.len() * 2);
        let mut rrect_cpu: Vec<RRectInst> = Vec::new();
        let mut glyph_cpu: Vec<GlyphInst> = Vec::with_capacity(cmds.len() * 4);
        let mut tri_fill_cpu: Vec<TriInst> = Vec::new();
        let mut tri_out_cpu: Vec<TriInst> = Vec::new();

        for cmd in cmds {
            match cmd {
                // ── Solid rects & lines ──────────────────────────────────────
                DrawCmd::FillRect { x, y, w, h, color } => {
                    if !color.is_transparent() {
                        bg_cpu.push(BgInst {
                            rect: [*x as f32, *y as f32, *w as f32, *h as f32],
                            color: color.to_f32(),
                        });
                    }
                }

                DrawCmd::StrokeRect { x, y, w, h, color } => {
                    let (xf, yf, wf, hf) = (*x as f32, *y as f32, *w as f32, *h as f32);
                    let c = color.to_f32();
                    for r in &[
                        [xf, yf, wf, 1.0],
                        [xf, yf + hf - 1.0, wf, 1.0],
                        [xf, yf, 1.0, hf],
                        [xf + wf - 1.0, yf, 1.0, hf],
                    ] {
                        bg_cpu.push(BgInst { rect: *r, color: c });
                    }
                }

                DrawCmd::HLine { x, y, w, color } => {
                    bg_cpu.push(BgInst {
                        rect: [*x as f32, *y as f32, *w as f32, 1.0],
                        color: color.to_f32(),
                    });
                }

                DrawCmd::VLine { x, y, h, color } => {
                    bg_cpu.push(BgInst {
                        rect: [*x as f32, *y as f32, 1.0, *h as f32],
                        color: color.to_f32(),
                    });
                }

                // ── BorderLine primitive ──────────────────────────────────────
                // Decomposed to BgInst rects — one per enabled side.
                DrawCmd::BorderLine {
                    x,
                    y,
                    w,
                    h,
                    sides,
                    color,
                    thickness,
                } => {
                    if color.is_transparent() {
                        continue;
                    }
                    let (xf, yf, wf, hf) = (*x as f32, *y as f32, *w as f32, *h as f32);
                    let t = (*thickness as f32).max(1.0);
                    let c = color.to_f32();
                    if sides.contains(BorderSide::TOP) {
                        bg_cpu.push(BgInst {
                            rect: [xf, yf, wf, t],
                            color: c,
                        });
                    }
                    if sides.contains(BorderSide::BOTTOM) {
                        bg_cpu.push(BgInst {
                            rect: [xf, yf + hf - t, wf, t],
                            color: c,
                        });
                    }
                    if sides.contains(BorderSide::LEFT) {
                        bg_cpu.push(BgInst {
                            rect: [xf, yf, t, hf],
                            color: c,
                        });
                    }
                    if sides.contains(BorderSide::RIGHT) {
                        bg_cpu.push(BgInst {
                            rect: [xf + wf - t, yf, t, hf],
                            color: c,
                        });
                    }
                }

                // ── RoundRect (SDF pass) ──────────────────────────────────────
                DrawCmd::RoundRect {
                    x,
                    y,
                    w,
                    h,
                    radii,
                    fill,
                    stroke,
                    stroke_w,
                } => {
                    if *w <= 0.0 || *h <= 0.0 {
                        continue;
                    }
                    rrect_cpu.push(RRectInst {
                        rect: [*x, *y, *w, *h],
                        radii: [radii.tl, radii.tr, radii.bl, radii.br],
                        fill: color_premul(*fill),
                        stroke: color_premul(*stroke),
                        strokew: *stroke_w,
                        _pad: [0.0; 3],
                    });
                }

                // ── PowerlineArrow (explicit primitive) ───────────────────────
                DrawCmd::PowerlineArrow {
                    x,
                    y,
                    w,
                    h,
                    dir,
                    color,
                } => {
                    let inst = TriInst {
                        rect: [*x as f32, *y as f32, *w as f32, *h as f32],
                        color: color.to_f32(),
                        dir: dir.as_f32(),
                        _pad: [0.0; 3],
                    };
                    if dir.is_filled() {
                        tri_fill_cpu.push(inst);
                    } else {
                        tri_out_cpu.push(inst);
                    }
                }

                // ── Text (TUI compat) ─────────────────────────────────────────
                DrawCmd::Text {
                    x,
                    y,
                    text,
                    style,
                    max_w,
                } => {
                    // Background rect
                    if !style.bg.is_transparent() {
                        let est_w = text.chars().count() as u32 * self.cell_w;
                        let bw = max_w.map(|m| m.min(est_w)).unwrap_or(est_w);
                        bg_cpu.push(BgInst {
                            rect: [*x as f32, *y as f32, bw as f32, self.cell_h as f32],
                            color: style.bg.to_f32(),
                        });
                    }

                    let needs_split = text.chars().any(|c| {
                        let cp = c as u32;
                        is_box_draw_cp(cp) || is_powerline_cp(cp)
                    });

                    if !needs_split {
                        self.shape_text_into(*x, *y, text, style, *max_w, &mut glyph_cpu);
                        continue;
                    }

                    // Per-character dispatch (TUI compat: box-draw / Powerline chars in text)
                    let fg = style.fg.to_f32();
                    let max_px = max_w.map(|m| m as i64);
                    let mut px_off: i64 = 0;
                    let mut run_start: i64 = 0;
                    let mut run_text = String::new();

                    let flush_run =
                        |run: &str,
                         run_px: i64,
                         atlas: &mut GlyphAtlas,
                         shaper: &Shaper,
                         hb_scale: f32,
                         cell_w: u32,
                         x: u32,
                         y: u32,
                         style: &TextStyle,
                         max_px: Option<i64>,
                         glyph_cpu: &mut Vec<GlyphInst>| {
                            if run.is_empty() {
                                return;
                            }
                            let run_max = max_px.map(|m| (m - run_px).max(0) as u32);
                            shape_run(
                                x + run_px as u32,
                                y,
                                run,
                                style,
                                run_max,
                                atlas,
                                shaper,
                                hb_scale,
                                cell_w,
                                glyph_cpu,
                            );
                        };

                    for ch in text.chars() {
                        if max_px.map_or(false, |m| px_off >= m) {
                            break;
                        }
                        let cell_x = *x + px_off as u32;
                        let cp = ch as u32;

                        if is_shade_block(ch) {
                            flush_run(
                                &run_text,
                                run_start,
                                &mut self.atlas,
                                &self.shaper,
                                self.hb_scale,
                                self.cell_w,
                                *x,
                                *y,
                                style,
                                max_px,
                                &mut glyph_cpu,
                            );
                            run_text.clear();
                            run_start = px_off + self.cell_w as i64;
                            let a = shade_alpha(ch);
                            let c = premul([fg[0], fg[1], fg[2], fg[3] * a]);
                            bg_cpu.push(BgInst {
                                rect: [
                                    cell_x as f32,
                                    *y as f32,
                                    self.cell_w as f32,
                                    self.cell_h as f32,
                                ],
                                color: c,
                            });
                            px_off += self.cell_w as i64;
                        } else if is_box_draw_cp(cp) {
                            flush_run(
                                &run_text,
                                run_start,
                                &mut self.atlas,
                                &self.shaper,
                                self.hb_scale,
                                self.cell_w,
                                *x,
                                *y,
                                style,
                                max_px,
                                &mut glyph_cpu,
                            );
                            run_text.clear();
                            run_start = px_off + self.cell_w as i64;
                            let segs = box_to_lines(ch, cell_x, *y, self.cell_w, self.cell_h);
                            if !segs.is_empty() {
                                for seg in segs {
                                    bg_cpu.push(BgInst {
                                        rect: seg,
                                        color: fg,
                                    });
                                }
                            } else if let Some(uv) = self.atlas.glyph(ch, style.bold, style.italic)
                            {
                                if uv.width > 0 {
                                    self.push_glyph(
                                        &uv,
                                        cell_x as f32,
                                        *y as f32,
                                        fg,
                                        &mut glyph_cpu,
                                    );
                                }
                            }
                            px_off += self.cell_w as i64;
                        } else if is_powerline_cp(cp) {
                            flush_run(
                                &run_text,
                                run_start,
                                &mut self.atlas,
                                &self.shaper,
                                self.hb_scale,
                                self.cell_w,
                                *x,
                                *y,
                                style,
                                max_px,
                                &mut glyph_cpu,
                            );
                            run_text.clear();
                            run_start = px_off + self.cell_w as i64;
                            let dir = powerline_dir_f32(ch);
                            let inst = TriInst {
                                rect: [
                                    cell_x as f32,
                                    *y as f32,
                                    self.cell_w as f32,
                                    self.cell_h as f32,
                                ],
                                color: fg,
                                dir,
                                _pad: [0.0; 3],
                            };
                            if dir < 2.0 {
                                tri_fill_cpu.push(inst);
                            } else {
                                tri_out_cpu.push(inst);
                            }
                            px_off += self.cell_w as i64;
                        } else {
                            run_text.push(ch);
                            px_off += self.cell_w as i64;
                        }
                    }

                    flush_run(
                        &run_text,
                        run_start,
                        &mut self.atlas,
                        &self.shaper,
                        self.hb_scale,
                        self.cell_w,
                        *x,
                        *y,
                        style,
                        max_px,
                        &mut glyph_cpu,
                    );
                }
            }
        }

        // Upload atlas patch if new glyphs were rasterised this frame.
        if self.atlas.dirty {
            unsafe {
                patch_atlas_tex(self.atlas_tex, &self.atlas);
            }
            self.atlas.dirty = false;
        }

        let (vw, vh) = (vp_w as f32, vp_h as f32);
        unsafe {
            // Own the viewport — the DRM compositor may have left it at the
            // raw output size, which won't match vp_w/vp_h after snapping.
            gl::Viewport(0, 0, vp_w as i32, vp_h as i32);
            gl::Enable(gl::BLEND);
            gl::BlendFuncSeparate(
                gl::ONE,
                gl::ONE_MINUS_SRC_ALPHA,
                gl::ONE,
                gl::ONE_MINUS_SRC_ALPHA,
            );
            let mut vp = [0i32; 4];
            gl::GetIntegerv(gl::VIEWPORT, vp.as_mut_ptr());
            let mut scissor_enabled = 0i32;
            gl::GetIntegerv(gl::SCISSOR_TEST, &mut scissor_enabled);
            let mut scissor = [0i32; 4];
            gl::GetIntegerv(gl::SCISSOR_BOX, scissor.as_mut_ptr());
            tracing::trace!(
                "GL viewport={vp:?} scissor_enabled={scissor_enabled} scissor={scissor:?}"
            );
            // ── Pass 1: BG quads ─────────────────────────────────────────────
            gl::UseProgram(self.bg_prog);
            gl::BindVertexArray(self.bg_vao);
            set_u2f(self.bg_prog, "u_vp", vw, vh);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.bg_ivbo);
            upload_inst(&bg_cpu, &mut self.bg_cap, size_of::<BgInst>());
            if !bg_cpu.is_empty() {
                gl::DrawArraysInstanced(gl::TRIANGLES, 0, 6, bg_cpu.len() as i32);
            }

            // ── Pass 2: RoundRect (SDF) ───────────────────────────────────────
            gl::UseProgram(self.rrect_prog);
            gl::BindVertexArray(self.rrect_vao);
            set_u2f(self.rrect_prog, "u_vp", vw, vh);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.rrect_ivbo);
            upload_inst(&rrect_cpu, &mut self.rrect_cap, size_of::<RRectInst>());
            if !rrect_cpu.is_empty() {
                gl::DrawArraysInstanced(gl::TRIANGLES, 0, 6, rrect_cpu.len() as i32);
            }

            // ── Pass 3: Glyph quads ───────────────────────────────────────────
            gl::UseProgram(self.glyph_prog);
            gl::BindVertexArray(self.glyph_vao);
            set_u2f(self.glyph_prog, "u_vp", vw, vh);
            set_u1i(self.glyph_prog, "u_atlas", 0);
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, self.atlas_tex);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.glyph_ivbo);
            upload_inst(&glyph_cpu, &mut self.glyph_cap, size_of::<GlyphInst>());
            if !glyph_cpu.is_empty() {
                gl::DrawArraysInstanced(gl::TRIANGLES, 0, 6, glyph_cpu.len() as i32);
            }

            // ── Pass 4: Powerline triangles ───────────────────────────────────
            gl::UseProgram(self.tri_prog);
            set_u2f(self.tri_prog, "u_vp", vw, vh);

            gl::BindVertexArray(self.tri_fill_vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.tri_fill_ivbo);
            upload_inst(&tri_fill_cpu, &mut self.tri_fill_cap, size_of::<TriInst>());
            if !tri_fill_cpu.is_empty() {
                gl::DrawArraysInstanced(gl::TRIANGLES, 0, 3, tri_fill_cpu.len() as i32);
            }

            gl::BindVertexArray(self.tri_out_vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.tri_out_ivbo);
            upload_inst(&tri_out_cpu, &mut self.tri_out_cap, size_of::<TriInst>());
            if !tri_out_cpu.is_empty() {
                gl::DrawArraysInstanced(gl::TRIANGLES, 0, 12, tri_out_cpu.len() as i32);
            }

            gl::BindVertexArray(0);
            gl::UseProgram(0);
        }
    }

    // ── Text shaping ──────────────────────────────────────────────────────────

    fn shape_text_into(
        &mut self,
        x: u32,
        y: u32,
        text: &str,
        style: &TextStyle,
        max_w: Option<u32>,
        out: &mut Vec<GlyphInst>,
    ) {
        let fg = style.fg.to_f32();
        let max_px = max_w.map(|m| m as f32);
        let cw_f = self.cell_w as f32;
        let mut px = x as f32;

        if run_is_atlas_synthetic(text) {
            for ch in text.chars() {
                if max_px.map_or(false, |m| px - x as f32 >= m) {
                    break;
                }
                if let Some(uv) = self.atlas.glyph(ch, style.bold, style.italic) {
                    if uv.width > 0 {
                        self.push_glyph(&uv, px, y as f32, fg, out);
                    }
                    px += uv.advance as f32;
                } else {
                    px += cw_f;
                }
            }
            return;
        }

        let shaped = self.shaper.shape(text);
        for sg in &shaped {
            if max_px.map_or(false, |m| px - x as f32 >= m) {
                break;
            }
            let adv = sg.cluster_width as f32 * cw_f;
            if let Some(uv) = self
                .atlas
                .glyph_by_id(sg.glyph_id, style.bold, style.italic)
            {
                if uv.width > 0 {
                    self.push_glyph(&uv, px, y as f32, fg, out);
                }
            }
            px += adv;
        }
    }

    #[inline]
    fn push_glyph(&self, uv: &GlyphUv, px: f32, py: f32, fg: [f32; 4], out: &mut Vec<GlyphInst>) {
        let pad = self.atlas.cell_h.saturating_sub(self.atlas.natural_h) / 2;
        let gx = px.round() + uv.bearing_x as f32;
        let gy = py.round() + pad as f32 + (self.ascender - uv.bearing_y) as f32;
        out.push(GlyphInst {
            glyph: [gx, gy, uv.width as f32, uv.height as f32],
            uv: [uv.uv_x, uv.uv_y, uv.uv_x + uv.uv_w, uv.uv_y + uv.uv_h],
            fg,
        });
    }
}

// ── shape_run ─────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn shape_run(
    x: u32,
    y: u32,
    text: &str,
    style: &TextStyle,
    max_w: Option<u32>,
    atlas: &mut GlyphAtlas,
    shaper: &Shaper,
    _hb_scale: f32,
    cell_w: u32,
    out: &mut Vec<GlyphInst>,
) {
    if text.is_empty() {
        return;
    }
    let fg = style.fg.to_f32();
    let max_px = max_w.map(|m| m as f32);
    let cw_f = cell_w as f32;
    let mut px = x as f32;
    let pad = atlas.cell_h.saturating_sub(atlas.natural_h) / 2;
    let ascender = atlas.ascender;
    let shaped = shaper.shape(text);

    for sg in &shaped {
        if max_px.map_or(false, |m| px - x as f32 >= m) {
            break;
        }
        let adv = sg.cluster_width as f32 * cw_f;
        if let Some(uv) = atlas.glyph_by_id(sg.glyph_id, style.bold, style.italic) {
            if uv.width > 0 {
                let gx = px.round() + uv.bearing_x as f32;
                let gy = y as f32 + pad as f32 + (ascender - uv.bearing_y) as f32;
                out.push(GlyphInst {
                    glyph: [gx, gy, uv.width as f32, uv.height as f32],
                    uv: [uv.uv_x, uv.uv_y, uv.uv_x + uv.uv_w, uv.uv_y + uv.uv_h],
                    fg,
                });
            }
        }
        px += adv;
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// GL helpers
// ═════════════════════════════════════════════════════════════════════════════

unsafe fn compile_prog(vert: &str, frag: &str) -> Result<u32, String> {
    let v = compile_shader(gl::VERTEX_SHADER, vert)?;
    let f = compile_shader(gl::FRAGMENT_SHADER, frag)?;
    let p = gl::CreateProgram();
    gl::AttachShader(p, v);
    gl::AttachShader(p, f);
    gl::LinkProgram(p);
    gl::DeleteShader(v);
    gl::DeleteShader(f);
    let mut ok = 0i32;
    gl::GetProgramiv(p, gl::LINK_STATUS, &mut ok);
    if ok == 0 {
        let mut len = 0i32;
        gl::GetProgramiv(p, gl::INFO_LOG_LENGTH, &mut len);
        let mut buf = vec![0u8; len as usize];
        gl::GetProgramInfoLog(p, len, std::ptr::null_mut(), buf.as_mut_ptr() as *mut _);
        gl::DeleteProgram(p);
        return Err(String::from_utf8_lossy(&buf).into_owned());
    }
    Ok(p)
}

unsafe fn compile_shader(kind: u32, src: &str) -> Result<u32, String> {
    let s = gl::CreateShader(kind);
    let c = CString::new(src).unwrap();
    gl::ShaderSource(s, 1, &c.as_ptr(), std::ptr::null());
    gl::CompileShader(s);
    let mut ok = 0i32;
    gl::GetShaderiv(s, gl::COMPILE_STATUS, &mut ok);
    if ok == 0 {
        let mut len = 0i32;
        gl::GetShaderiv(s, gl::INFO_LOG_LENGTH, &mut len);
        let mut buf = vec![0u8; len as usize];
        gl::GetShaderInfoLog(s, len, std::ptr::null_mut(), buf.as_mut_ptr() as *mut _);
        gl::DeleteShader(s);
        return Err(String::from_utf8_lossy(&buf).into_owned());
    }
    Ok(s)
}

unsafe fn create_bg_vao(prog: u32, cap: usize) -> (u32, u32) {
    let (mut vao, mut qvbo, mut ivbo) = (0u32, 0u32, 0u32);
    gl::GenVertexArrays(1, &mut vao);
    gl::GenBuffers(1, &mut qvbo);
    gl::GenBuffers(1, &mut ivbo);
    gl::BindVertexArray(vao);
    gl::BindBuffer(gl::ARRAY_BUFFER, qvbo);
    gl::BufferData(
        gl::ARRAY_BUFFER,
        (QUAD.len() * 4) as isize,
        QUAD.as_ptr() as *const _,
        gl::STATIC_DRAW,
    );
    let a = attr_loc(prog, "a_pos");
    gl::EnableVertexAttribArray(a);
    gl::VertexAttribPointer(a, 2, gl::FLOAT, gl::FALSE, 8, std::ptr::null());
    gl::BindBuffer(gl::ARRAY_BUFFER, ivbo);
    gl::BufferData(
        gl::ARRAY_BUFFER,
        (cap * size_of::<BgInst>()) as isize,
        std::ptr::null(),
        gl::DYNAMIC_DRAW,
    );
    let s = size_of::<BgInst>() as i32;
    inst_attr(prog, "i_rect", 4, 0, s);
    inst_attr(prog, "i_color", 4, 16, s);
    gl::BindVertexArray(0);
    (vao, ivbo)
}

/// RoundRect SDF VAO.
///
/// Instance layout mirrors `RRectInst` (96 bytes):
///   offset  0 — rect    vec4
///   offset 16 — radii   vec4
///   offset 32 — fill    vec4
///   offset 48 — stroke  vec4
///   offset 64 — strokew f32
///   offset 68 — pad     [f32; 3]
unsafe fn create_rrect_vao(prog: u32, cap: usize) -> (u32, u32) {
    let (mut vao, mut qvbo, mut ivbo) = (0u32, 0u32, 0u32);
    gl::GenVertexArrays(1, &mut vao);
    gl::GenBuffers(1, &mut qvbo);
    gl::GenBuffers(1, &mut ivbo);
    gl::BindVertexArray(vao);
    gl::BindBuffer(gl::ARRAY_BUFFER, qvbo);
    gl::BufferData(
        gl::ARRAY_BUFFER,
        (QUAD.len() * 4) as isize,
        QUAD.as_ptr() as *const _,
        gl::STATIC_DRAW,
    );
    let a = attr_loc(prog, "a_pos");
    gl::EnableVertexAttribArray(a);
    gl::VertexAttribPointer(a, 2, gl::FLOAT, gl::FALSE, 8, std::ptr::null());
    gl::BindBuffer(gl::ARRAY_BUFFER, ivbo);
    gl::BufferData(
        gl::ARRAY_BUFFER,
        (cap * size_of::<RRectInst>()) as isize,
        std::ptr::null(),
        gl::DYNAMIC_DRAW,
    );
    let s = size_of::<RRectInst>() as i32;
    inst_attr(prog, "i_rect", 4, 0, s);
    inst_attr(prog, "i_radii", 4, 16, s);
    inst_attr(prog, "i_fill", 4, 32, s);
    inst_attr(prog, "i_stroke", 4, 48, s);
    // i_strokew: single float at offset 64
    let loc = attr_loc(prog, "i_strokew");
    gl::EnableVertexAttribArray(loc);
    gl::VertexAttribPointer(loc, 1, gl::FLOAT, gl::FALSE, s, 64 as *const _);
    gl::VertexAttribDivisor(loc, 1);
    gl::BindVertexArray(0);
    (vao, ivbo)
}

unsafe fn create_glyph_vao(prog: u32, cap: usize) -> (u32, u32) {
    let (mut vao, mut qvbo, mut ivbo) = (0u32, 0u32, 0u32);
    gl::GenVertexArrays(1, &mut vao);
    gl::GenBuffers(1, &mut qvbo);
    gl::GenBuffers(1, &mut ivbo);
    gl::BindVertexArray(vao);
    gl::BindBuffer(gl::ARRAY_BUFFER, qvbo);
    gl::BufferData(
        gl::ARRAY_BUFFER,
        (QUAD.len() * 4) as isize,
        QUAD.as_ptr() as *const _,
        gl::STATIC_DRAW,
    );
    let a = attr_loc(prog, "a_pos");
    gl::EnableVertexAttribArray(a);
    gl::VertexAttribPointer(a, 2, gl::FLOAT, gl::FALSE, 8, std::ptr::null());
    gl::BindBuffer(gl::ARRAY_BUFFER, ivbo);
    gl::BufferData(
        gl::ARRAY_BUFFER,
        (cap * size_of::<GlyphInst>()) as isize,
        std::ptr::null(),
        gl::DYNAMIC_DRAW,
    );
    let s = size_of::<GlyphInst>() as i32;
    inst_attr(prog, "i_glyph", 4, 0, s);
    inst_attr(prog, "i_uv", 4, 16, s);
    inst_attr(prog, "i_fg", 4, 32, s);
    gl::BindVertexArray(0);
    (vao, ivbo)
}

unsafe fn create_tri_vao(prog: u32, cap: usize) -> (u32, u32) {
    let (mut vao, mut ivbo) = (0u32, 0u32);
    gl::GenVertexArrays(1, &mut vao);
    gl::GenBuffers(1, &mut ivbo);
    gl::BindVertexArray(vao);
    gl::BindBuffer(gl::ARRAY_BUFFER, ivbo);
    gl::BufferData(
        gl::ARRAY_BUFFER,
        (cap * size_of::<TriInst>()) as isize,
        std::ptr::null(),
        gl::DYNAMIC_DRAW,
    );
    let s = size_of::<TriInst>() as i32;
    inst_attr(prog, "i_rect", 4, 0, s);
    inst_attr(prog, "i_color", 4, 16, s);
    let loc = attr_loc(prog, "i_dir");
    gl::EnableVertexAttribArray(loc);
    gl::VertexAttribPointer(loc, 1, gl::FLOAT, gl::FALSE, s, 32 as *const _);
    gl::VertexAttribDivisor(loc, 1);
    gl::BindVertexArray(0);
    (vao, ivbo)
}

unsafe fn upload_atlas_tex(atlas: &GlyphAtlas) -> u32 {
    let dim = ATLAS_DIM as i32;
    let mut tex = 0u32;
    gl::GenTextures(1, &mut tex);
    gl::BindTexture(gl::TEXTURE_2D, tex);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
    gl::TexImage2D(
        gl::TEXTURE_2D,
        0,
        gl::RGBA8 as i32,
        dim,
        dim,
        0,
        gl::RGBA,
        gl::UNSIGNED_BYTE,
        atlas.pixels.as_ptr() as *const _,
    );
    tex
}

unsafe fn patch_atlas_tex(tex: u32, atlas: &GlyphAtlas) {
    let dim = ATLAS_DIM as i32;
    let rows = (atlas.cursor_y + atlas.row_h + 1).min(ATLAS_DIM) as i32;
    gl::BindTexture(gl::TEXTURE_2D, tex);
    gl::TexSubImage2D(
        gl::TEXTURE_2D,
        0,
        0,
        0,
        dim,
        rows,
        gl::RGBA,
        gl::UNSIGNED_BYTE,
        atlas.pixels.as_ptr() as *const _,
    );
}

unsafe fn upload_inst<T: Copy>(data: &[T], cap: &mut usize, item_sz: usize) {
    if data.len() > *cap {
        let nc = (data.len() * 2).max(64);
        gl::BufferData(
            gl::ARRAY_BUFFER,
            (nc * item_sz) as isize,
            std::ptr::null(),
            gl::DYNAMIC_DRAW,
        );
        *cap = nc;
    }
    if !data.is_empty() {
        gl::BufferSubData(
            gl::ARRAY_BUFFER,
            0,
            (data.len() * item_sz) as isize,
            data.as_ptr() as *const _,
        );
    }
}

fn attr_loc(prog: u32, name: &str) -> u32 {
    let c = CString::new(name).unwrap();
    unsafe { gl::GetAttribLocation(prog, c.as_ptr()) as u32 }
}

unsafe fn inst_attr(prog: u32, name: &str, size: i32, offset: i32, stride: i32) {
    let loc = attr_loc(prog, name);
    gl::EnableVertexAttribArray(loc);
    gl::VertexAttribPointer(loc, size, gl::FLOAT, gl::FALSE, stride, offset as *const _);
    gl::VertexAttribDivisor(loc, 1);
}

fn set_u2f(prog: u32, name: &str, x: f32, y: f32) {
    let c = CString::new(name).unwrap();
    unsafe {
        gl::Uniform2f(gl::GetUniformLocation(prog, c.as_ptr()), x, y);
    }
}

fn set_u1i(prog: u32, name: &str, v: i32) {
    let c = CString::new(name).unwrap();
    unsafe {
        gl::Uniform1i(gl::GetUniformLocation(prog, c.as_ptr()), v);
    }
}
