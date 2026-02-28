//! gl.rs — OpenGL ES 3 renderer.
//!
//! Consumes a `Vec<DrawCmd>` and flushes it to the currently-bound FBO via
//! two instanced draw calls: one for background quads, one for glyph quads.
//!
//! NDC conversion — THE ONLY PLACE it lives:
//!   ndc.x =  (px.x / vp.x) * 2.0 - 1.0
//!   ndc.y = -(px.y / vp.y) * 2.0 + 1.0   ← Y flip: pixel 0 = NDC top
//!
//! Never do NDC math outside these shaders.
//!
//! ── Font system ───────────────────────────────────────────────────────────────
//!
//! This file embeds trixie's full font pipeline:
//!
//!   • Three ab_glyph faces: regular, bold, italic (with fallback chain)
//!   • rustybuzz HarfBuzz shaping for ligatures via `Shaper`
//!   • Explicit atlas preload for box-drawing / braille / Powerline glyphs
//!   • Correct `glyph_by_id` using font-internal glyph indices (not Unicode)
//!
//! `ChromeRenderer` uses `shape_text_into` which runs the full shaping
//! pipeline on every text run, collapsing ligature clusters automatically.
//!
//! To construct:
//!
//!   let atlas = GlyphAtlas::new(
//!       regular_bytes,
//!       Some(bold_bytes),
//!       Some(italic_bytes),
//!       size_px,
//!       1.0,   // line_spacing reserved, pass 1.0
//!   )?;
//!   let shaper = Shaper::new(Box::leak(regular_bytes.into_boxed_slice()));
//!   let renderer = ChromeRenderer::new(atlas, shaper)?;

use std::collections::HashMap;
use std::ffi::CString;

use ab_glyph::{Font, FontRef, GlyphId, PxScale, ScaleFont};
use rustybuzz::{Face, UnicodeBuffer};

use super::{Color, DrawCmd, TextStyle};

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
    vec2 ndc = (px / u_vp) * 2.0 - 1.0;
    // No Y-flip here — caller is responsible for coordinate convention.
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
    vec2 ndc = (px / u_vp) * 2.0 - 1.0;
    // No Y-flip here — caller is responsible for coordinate convention.
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
    // Boost coverage to compensate for linear->perceptual thinning.
    // Adjust the exponent: lower = bolder (0.5 is a good start).
    a = pow(a, 0.5);
    float alpha = v_fg.a * a;
    fragColor = vec4(v_fg.rgb * alpha, alpha);
}
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
struct GlyphInst {
    glyph: [f32; 4],
    uv: [f32; 4],
    fg: [f32; 4],
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

/// UV coordinates and metrics for one glyph in the atlas.
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

// Cache key for char-based lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CharKey {
    ch: char,
    bold: bool,
    italic: bool,
}

// Cache key for HarfBuzz glyph-ID lookups (shaped / ligature path).
// These are font-internal indices, NOT Unicode codepoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct IdKey {
    id: u16,
    bold: bool,
    italic: bool,
}

// Owned font bytes + parsed ab_glyph handle in one allocation.
// The `FontRef<'static>` is safe because `_bytes` is pinned in the Box and
// we never expose the 'static reference outside this module.
struct OwnedFace {
    _bytes: Vec<u8>,
    font: FontRef<'static>,
    scale: PxScale,
}

impl OwnedFace {
    fn new(data: Vec<u8>, scale: PxScale) -> Result<Self, String> {
        let font: FontRef<'static> = unsafe {
            let slice: &[u8] = &data;
            let extended: &'static [u8] = &*(slice as *const [u8]);
            FontRef::try_from_slice(extended).map_err(|e| format!("ab_glyph parse error: {e}"))?
        };
        Ok(Self {
            _bytes: data,
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
/// Supports three font faces (regular / bold / italic) with correct
/// HarfBuzz glyph-ID lookup and explicit preloading of box-drawing,
/// braille, and Powerline codepoint ranges.
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
    pub natural_h: u32, // cell_h before padding
    pub ascender: i32,
    pub dirty: bool,
}

impl GlyphAtlas {
    pub fn new(
        regular_data: &[u8],
        bold_data: Option<&[u8]>,
        italic_data: Option<&[u8]>,
        size_px: f32,
        _line_spacing: f32, // reserved for future inter-line padding control
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

        tracing::info!(
            "GlyphAtlas: regular=ok bold={} italic={}",
            bold.is_some(),
            italic.is_some(),
        );

        let sf = regular.scaled();

        // ab_glyph ascent() / descent() are in pixels at the requested scale.
        // descent() is negative (below baseline), so we take its absolute value.
        let ascender_f = sf.ascent();
        let descender_f = sf.descent().abs();
        let line_gap = sf.line_gap();

        // Natural line height from font metrics. We add a fixed 3px layout
        // padding so that rows*cell_h never lands exactly on the viewport
        // boundary (which causes the last row to be clipped by the GPU).
        // Glyphs are vertically centred within the padded cell; box-drawing
        // characters bypass the centering and fill the full cell_h so they
        // connect edge-to-edge.
        let natural_h = ascender_f + descender_f + line_gap;
        let cell_h_raw = natural_h.ceil() as u32;
        let cell_h = (cell_h_raw + 3).max(1);

        // ceil() so tall ascenders (Iosevka's caps) are never clipped.
        let ascender = ascender_f.ceil() as i32;

        // Cell width from the advance of '0'. For Iosevka this is the correct
        // monospace advance; we do not round up to avoid inflating cell width.
        let cell_w = {
            let id = sf.font.glyph_id('0');
            let adv = sf.h_advance(id).ceil() as u32;
            if adv > 0 {
                adv
            } else {
                let sid = sf.font.glyph_id(' ');
                let sadv = sf.h_advance(sid).ceil() as u32;
                if sadv > 0 {
                    sadv
                } else {
                    size_px as u32
                }
            }
            .max(4)
        };

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
            natural_h: cell_h_raw,
            cell_w,
            cell_h,
            ascender,
            dirty: true,
        };

        tracing::info!(
            "GlyphAtlas: {size_px:.1}px cell={cell_w}×{cell_h} natural_h={cell_h_raw} ascender={ascender}",
        );

        // Warm-up: ASCII × {regular, bold, italic}
        for ch in ' '..='~' {
            atlas.glyph(ch, false, false);
            atlas.glyph(ch, true, false);
            atlas.glyph(ch, false, true);
        }
        // Box-drawing & block elements
        for cp in 0x2500u32..=0x259F {
            if let Some(ch) = char::from_u32(cp) {
                atlas.glyph(ch, false, false);
            }
        }
        // Braille patterns
        for cp in 0x2800u32..=0x28FF {
            if let Some(ch) = char::from_u32(cp) {
                atlas.glyph(ch, false, false);
            }
        }
        // Powerline / Nerd Font arrows
        for cp in [0xE0B0u32, 0xE0B1, 0xE0B2, 0xE0B3] {
            if let Some(ch) = char::from_u32(cp) {
                atlas.glyph(ch, false, false);
            }
        }
        Ok(atlas)
    }

    // ── char-based lookup ─────────────────────────────────────────────────────

    pub fn glyph(&mut self, ch: char, bold: bool, italic: bool) -> Option<GlyphUv> {
        let key = CharKey { ch, bold, italic };
        if let Some(&cached) = self.char_cache.get(&key) {
            return cached;
        }
        let info = self.rasterise_char(ch, bold, italic);
        self.char_cache.insert(key, info);
        info
    }

    // ── HarfBuzz glyph-ID lookup ──────────────────────────────────────────────

    pub fn glyph_by_id(&mut self, id: u16, bold: bool, italic: bool) -> Option<GlyphUv> {
        let key = IdKey { id, bold, italic };
        if let Some(&cached) = self.id_cache.get(&key) {
            return cached;
        }
        let info = self.rasterise_by_id(id, bold, italic);
        self.id_cache.insert(key, info);
        info
    }

    // ── internal rasterisers ──────────────────────────────────────────────────

    fn rasterise_char(&mut self, ch: char, bold: bool, italic: bool) -> Option<GlyphUv> {
        let face_ptr = self.pick_face(bold, italic);
        let glyph_id = unsafe { (*face_ptr).scaled().font.glyph_id(ch) };

        // Fall back to regular if the chosen variant doesn't have this glyph.
        let final_ptr = if glyph_id == GlyphId(0) && (bold || italic) {
            &self.regular as *const OwnedFace
        } else {
            face_ptr
        };

        let final_id = if final_ptr != face_ptr {
            unsafe { (*final_ptr).scaled().font.glyph_id(ch) }
        } else {
            glyph_id
        };

        self.rasterise_from_ptr(final_id, final_ptr)
    }

    fn rasterise_by_id(&mut self, id: u16, bold: bool, italic: bool) -> Option<GlyphUv> {
        let face_ptr = self.pick_face(bold, italic);
        self.rasterise_from_ptr(GlyphId(id), face_ptr)
    }

    fn pick_face(&self, bold: bool, italic: bool) -> *const OwnedFace {
        if bold && self.bold.is_some() {
            self.bold.as_ref().unwrap() as *const _
        } else if italic && self.italic.is_some() {
            self.italic.as_ref().unwrap() as *const _
        } else {
            &self.regular as *const _
        }
    }

    fn rasterise_from_ptr(
        &mut self,
        glyph_id: GlyphId,
        face_ptr: *const OwnedFace,
    ) -> Option<GlyphUv> {
        let sf = unsafe { (*face_ptr).scaled() };
        let advance = sf.h_advance(glyph_id).ceil() as u32;
        let glyph = glyph_id.with_scale_and_position(sf.scale, ab_glyph::point(0.0, sf.ascent()));
        let outlined = sf.font.outline_glyph(glyph);
        drop(sf);

        let Some(outlined) = outlined else {
            return Some(GlyphUv {
                uv_x: 0.0,
                uv_y: 0.0,
                uv_w: 0.0,
                uv_h: 0.0,
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
        // bearing_x: floor so we don't clip the left edge of glyphs.
        // bearing_y: the distance from the baseline to the top of the bitmap.
        //   px_bounds().min.y is the *top* of the glyph in ab_glyph's
        //   Y-down space (it is negative for glyphs above the baseline).
        //   We negate it to get a positive "pixels above baseline" value.
        let bearing_x = bounds.min.x.floor() as i32;
        let bearing_y = (-bounds.min.y).ceil() as i32;

        if w == 0 || h == 0 {
            return Some(GlyphUv {
                uv_x: 0.0,
                uv_y: 0.0,
                uv_w: 0.0,
                uv_h: 0.0,
                width: 0,
                height: 0,
                bearing_x,
                bearing_y,
                advance,
            });
        }

        let mut coverage = vec![0u8; (w * h) as usize];
        outlined.draw(|px, py, cov| {
            let idx = (py * w + px) as usize;
            if idx < coverage.len() {
                coverage[idx] = (cov * 255.0).round() as u8;
            }
        });

        self.blit(coverage, w, h, bearing_x, bearing_y, advance)
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

        // With LINEAR atlas filtering we no longer need the half-texel inset
        // to prevent NEAREST bleed, but keep a smaller inset (0.25 texel) to
        // avoid any residual interpolation bleeding from adjacent glyphs.
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
// Shaper  (rustybuzz HarfBuzz shaping)
// ═════════════════════════════════════════════════════════════════════════════

/// A single output glyph from a shaped text run.
#[derive(Debug, Clone)]
pub struct ShapedGlyph {
    /// Font-internal glyph index (NOT a Unicode codepoint).
    pub glyph_id: u16,
    /// How many input *characters* this glyph consumed.
    /// >1 for ligatures (e.g. "=>" → single glyph, cluster_width=2).
    pub cluster_width: usize,
    /// Horizontal advance in the atlas's pixel units.
    pub advance_px: f32,
}

/// HarfBuzz text shaper. One instance per font face.
///
/// `font_data` must be `'static` — call
/// `Box::leak(bytes.into_boxed_slice())` before constructing.
pub struct Shaper {
    face: Face<'static>,
}

impl Shaper {
    pub fn new(font_data: &'static [u8]) -> Self {
        let face = Face::from_slice(font_data, 0).expect("rustybuzz: failed to parse font face");
        Self { face }
    }

    /// Shape a single visual run (homogeneous style, no newlines).
    pub fn shape(&self, text: &str) -> Vec<ShapedGlyph> {
        if text.is_empty() {
            return vec![];
        }

        let mut buf = UnicodeBuffer::new();
        buf.push_str(text);
        // Auto-detect script/language — do NOT pre-specify LATIN; that panics
        // when the face reports a different script (e.g. Zzzz).
        let output = rustybuzz::shape(&self.face, &[], buf);

        let positions = output.glyph_positions();
        let infos = output.glyph_infos();

        let mut result = Vec::with_capacity(infos.len());
        for i in 0..infos.len() {
            let cluster_byte = infos[i].cluster as usize;
            let next_cluster_byte = infos
                .get(i + 1)
                .map(|g| g.cluster as usize)
                .unwrap_or(text.len());

            let cluster_width = text[cluster_byte..next_cluster_byte].chars().count().max(1);

            // HarfBuzz x_advance is in font units; convert to pixels using
            // the same scale factor as ab_glyph (units_per_em → size_px).
            // We store the raw value here and scale in shape_text_into.
            let advance_px = positions[i].x_advance as f32;

            result.push(ShapedGlyph {
                glyph_id: infos[i].glyph_id as u16,
                cluster_width,
                advance_px,
            });
        }
        result
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// ChromeRenderer
// ═════════════════════════════════════════════════════════════════════════════

/// OpenGL ES 3 renderer. One instance per GL context.
///
/// Call `flush()` once per frame after building your `Vec<DrawCmd>`.
pub struct ChromeRenderer {
    bg_prog: u32,
    bg_vao: u32,
    bg_ivbo: u32,
    bg_cap: usize,

    glyph_prog: u32,
    glyph_vao: u32,
    glyph_ivbo: u32,
    glyph_cap: usize,

    atlas_tex: u32,
    pub atlas: GlyphAtlas,

    shaper: Shaper,

    /// Scaling factor: HarfBuzz font-unit → pixel.
    /// = size_px / units_per_em
    hb_scale: f32,

    pub cell_w: u32,
    pub cell_h: u32,
    ascender: i32,
}

impl ChromeRenderer {
    /// Create after the GL context is current and `gl::load_with` has been called.
    ///
    /// `shaper_font_data` must be `'static` (see `Shaper::new`).
    /// `hb_units_per_em` is the value from the font's head table
    /// (typically 1000 or 2048); used to scale HarfBuzz advances to pixels.
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
        let glyph_prog = unsafe { compile_prog(GLYPH_VERT, GLYPH_FRAG)? };

        let (bg_vao, bg_ivbo) = unsafe { create_bg_vao(bg_prog, 1024) };
        let (glyph_vao, glyph_ivbo) = unsafe { create_glyph_vao(glyph_prog, 4096) };
        let atlas_tex = unsafe { upload_atlas_tex(&atlas) };

        Ok(Self {
            bg_prog,
            bg_vao,
            bg_ivbo,
            bg_cap: 1024,
            glyph_prog,
            glyph_vao,
            glyph_ivbo,
            glyph_cap: 4096,
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
    ///
    /// `vp_w` / `vp_h` must be the physical pixel dimensions of the FBO.
    pub fn flush(&mut self, cmds: &[DrawCmd], vp_w: u32, vp_h: u32) {
        if cmds.is_empty() || vp_w == 0 || vp_h == 0 {
            return;
        }

        let mut bg_cpu: Vec<BgInst> = Vec::with_capacity(cmds.len() * 2);
        let mut glyph_cpu: Vec<GlyphInst> = Vec::with_capacity(cmds.len() * 4);

        for cmd in cmds {
            match cmd {
                DrawCmd::FillRect { x, y, w, h, color } => {
                    if !color.is_transparent() {
                        bg_cpu.push(BgInst {
                            rect: [*x as f32, *y as f32, *w as f32, *h as f32],
                            color: color.to_f32(),
                        });
                    }
                }

                DrawCmd::StrokeRect { x, y, w, h, color } => {
                    let (x, y, w, h) = (*x as f32, *y as f32, *w as f32, *h as f32);
                    let c = color.to_f32();
                    for r in &[
                        [x, y, w, 1.0],
                        [x, y + h - 1.0, w, 1.0],
                        [x, y, 1.0, h],
                        [x + w - 1.0, y, 1.0, h],
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
                    // Shaped glyph quads
                    self.shape_text_into(*x, *y, text, style, *max_w, &mut glyph_cpu);
                }
            }
        }

        // Upload atlas patch if any new glyphs were rasterised this frame.
        if self.atlas.dirty {
            unsafe {
                patch_atlas_tex(self.atlas_tex, &self.atlas);
            }
            self.atlas.dirty = false;
        }

        let (vw, vh) = (vp_w as f32, vp_h as f32);
        unsafe {
            gl::Enable(gl::BLEND);
            gl::BlendFuncSeparate(
                gl::ONE,
                gl::ONE_MINUS_SRC_ALPHA,
                gl::ONE,
                gl::ONE_MINUS_SRC_ALPHA,
            );

            // Background quads
            gl::UseProgram(self.bg_prog);
            gl::BindVertexArray(self.bg_vao);
            set_u2f(self.bg_prog, "u_vp", vw, vh);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.bg_ivbo);
            upload_inst(&bg_cpu, &mut self.bg_cap, std::mem::size_of::<BgInst>());
            if !bg_cpu.is_empty() {
                gl::DrawArraysInstanced(gl::TRIANGLES, 0, 6, bg_cpu.len() as i32);
            }

            // Glyph quads
            gl::UseProgram(self.glyph_prog);
            gl::BindVertexArray(self.glyph_vao);
            set_u2f(self.glyph_prog, "u_vp", vw, vh);
            set_u1i(self.glyph_prog, "u_atlas", 0);
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, self.atlas_tex);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.glyph_ivbo);
            upload_inst(
                &glyph_cpu,
                &mut self.glyph_cap,
                std::mem::size_of::<GlyphInst>(),
            );
            if !glyph_cpu.is_empty() {
                gl::DrawArraysInstanced(gl::TRIANGLES, 0, 6, glyph_cpu.len() as i32);
            }

            gl::BindVertexArray(0);
            gl::UseProgram(0);
        }
    }

    // ── text shaping ─────────────────────────────────────────────────────────
    //
    // Runs the full HarfBuzz pipeline for every text cmd. Ligature clusters
    // advance by `cluster_width * cell_w` in pixel space so the TWM cell
    // grid stays aligned even when glyphs collapse.

    fn shape_text_into(
        &mut self,
        x: u32,
        y: u32,
        text: &str,
        style: &TextStyle,
        max_w: Option<u32>,
        out: &mut Vec<GlyphInst>,
    ) {
        // Skip HarfBuzz for box-drawing / braille / Powerline — these are
        // synthetic glyphs that char-based lookup handles perfectly and
        // shaping would produce identity clusters anyway.
        let is_synthetic = text.chars().all(|c| is_synthetic_cp(c as u32));

        let fg = style.fg.to_f32();
        let max_px = max_w.map(|m| m as f32);
        let cell_w_f = self.cell_w as f32;
        let mut px = x as f32;

        if is_synthetic {
            for ch in text.chars() {
                if max_px.map_or(false, |m| px - x as f32 >= m) {
                    break;
                }
                if let Some(uv) = self.atlas.glyph(ch, style.bold, style.italic) {
                    if uv.width > 0 && uv.height > 0 {
                        self.push_glyph(&uv, px, y as f32, fg, Some(ch), out);
                    }
                    px += uv.advance as f32;
                } else {
                    px += cell_w_f;
                }
            }
            return;
        }

        // Full HarfBuzz shaping path.
        let shaped = self.shaper.shape(text);

        for sg in &shaped {
            if max_px.map_or(false, |m| px - x as f32 >= m) {
                break;
            }

            // Advance is cluster_width cells — keeps grid alignment for ligatures.
            let glyph_advance = sg.cluster_width as f32 * cell_w_f;

            if let Some(uv) = self
                .atlas
                .glyph_by_id(sg.glyph_id, style.bold, style.italic)
            {
                if uv.width > 0 && uv.height > 0 {
                    // For shaped glyphs we don't have the original char easily,
                    // but HarfBuzz path is only used for non-synthetic text so
                    // box-drawing never reaches here. Pass None.
                    self.push_glyph(&uv, px, y as f32, fg, None, out);
                }
            }

            px += glyph_advance;
        }
    }

    #[inline]
    fn push_glyph(
        &self,
        uv: &GlyphUv,
        px: f32,
        py: f32,
        fg: [f32; 4],
        ch: Option<char>,
        out: &mut Vec<GlyphInst>,
    ) {
        // Box-drawing, block elements and Powerline glyphs must fill the full
        // cell height so consecutive characters connect edge-to-edge.
        // All other glyphs are vertically centred within the padded cell.
        let is_box = ch.map_or(false, |c| {
            let cp = c as u32;
            matches!(cp,
                0x2500..=0x257F |  // box drawing
                0x2580..=0x259F |  // block elements
                0x2800..=0x28FF |  // braille
                0xE0B0 | 0xE0B1 | 0xE0B2 | 0xE0B3  // Powerline
            )
        });
        let pad = if is_box {
            0u32
        } else {
            self.atlas.cell_h.saturating_sub(self.atlas.natural_h) / 2
        };
        let gx = px.round() + uv.bearing_x as f32;
        let gy = py.round() + pad as f32 + (self.ascender - uv.bearing_y) as f32;
        out.push(GlyphInst {
            glyph: [gx, gy, uv.width as f32, uv.height as f32],
            uv: [uv.uv_x, uv.uv_y, uv.uv_x + uv.uv_w, uv.uv_y + uv.uv_h],
            fg,
        });
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Synthetic codepoint check
// ═════════════════════════════════════════════════════════════════════════════

#[inline]
fn is_synthetic_cp(cp: u32) -> bool {
    matches!(cp,
        0x2500..=0x257F |  // box drawing
        0x2580..=0x259F |  // block elements
        0x2800..=0x28FF |  // braille
        0xE0B0 | 0xE0B1 | 0xE0B2 | 0xE0B3  // Powerline
    )
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
        (cap * std::mem::size_of::<BgInst>()) as isize,
        std::ptr::null(),
        gl::DYNAMIC_DRAW,
    );
    let s = std::mem::size_of::<BgInst>() as i32;
    inst_attr(prog, "i_rect", 4, 0, s);
    inst_attr(prog, "i_color", 4, 16, s);

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
        (cap * std::mem::size_of::<GlyphInst>()) as isize,
        std::ptr::null(),
        gl::DYNAMIC_DRAW,
    );
    let s = std::mem::size_of::<GlyphInst>() as i32;
    inst_attr(prog, "i_glyph", 4, 0, s);
    inst_attr(prog, "i_uv", 4, 16, s);
    inst_attr(prog, "i_fg", 4, 32, s);

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
        let new_cap = (data.len() * 2).max(64);
        gl::BufferData(
            gl::ARRAY_BUFFER,
            (new_cap * item_sz) as isize,
            std::ptr::null(),
            gl::DYNAMIC_DRAW,
        );
        *cap = new_cap;
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
