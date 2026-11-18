//! The Glass theme's backdrop and sheets.
//!
//! egui cannot blur what lies under a shape, so the frost comes from what
//! is behind it instead: the backdrop is nothing but a dark base and a few
//! wide, soft pools of light. A translucent sheet over light that has no
//! detail looks the same as a blurred one, which is the whole trick. Pages,
//! side panels and the player bar are then thin white sheets over it, edged
//! with hairlines, as Raycast draws its windows.
//!
//! The brightest pool takes the playing cover's colour when colours come
//! from album art, and eases to the next cover's as songs change. The
//! backdrop is still otherwise: it never asks for a frame of its own.
//!
//! How strong the glass is comes from Settings as a percentage: the pools
//! follow it fully, while the sheets follow it by half, so rows and
//! buttons stay visible at the weakest setting.

use egui::{Color32, Mesh, Rect, Stroke, pos2, vec2};

use super::Palette;

/// The sidebar's, the right-hand panels' and the player bar's sheet.
pub const CHROME: Color32 = Color32::from_rgba_premultiplied(14, 14, 14, 14);
/// The hairline where a sheet meets the page.
pub const EDGE: Color32 = Color32::from_rgba_premultiplied(26, 26, 26, 26);

/// Raycast's coral: the brightest pool when no cover colours it.
const CORAL: Color32 = Color32::from_rgb(0xff, 0x63, 0x63);
const VIOLET: Color32 = Color32::from_rgb(0x6e, 0x56, 0xcf);
const BLUE: Color32 = Color32::from_rgb(0x2f, 0x6b, 0xff);
/// The base's top, a shade above the window it fades into.
const BASE_TOP: Color32 = Color32::from_rgb(0x10, 0x10, 0x15);

/// The glass strength Settings starts at, and the range it offers, in
/// percent of the theme as designed.
pub const DEFAULT_PERCENT: u8 = 100;
pub const MIN_PERCENT: u8 = 25;
pub const MAX_PERCENT: u8 = 200;

/// A strength from Settings' percentage, kept inside the range offered.
pub fn strength(percent: u8) -> f32 {
    f32::from(percent.clamp(MIN_PERCENT, MAX_PERCENT)) / 100.0
}

/// How much a sheet's opacity scales at `strength`: by half as much as
/// the light, so a sheet never vanishes nor turns opaque.
pub fn sheet_scale(strength: f32) -> f32 {
    0.5 + 0.5 * strength
}

/// A translucent white sheet of `alpha` at `strength`.
pub fn sheet(alpha: u8, strength: f32) -> Color32 {
    Color32::from_white_alpha(alpha).gamma_multiply(sheet_scale(strength))
}

/// The chrome sheet at `strength`.
pub fn chrome(strength: f32) -> Color32 {
    CHROME.gamma_multiply(sheet_scale(strength))
}

/// How long the lead pool takes to move to the next cover's colour.
const GLOW_FADE_SECONDS: f32 = 0.9;
/// The rings a pool is built from, and the points around each.
const RINGS: usize = 10;
const SEGMENTS: usize = 64;

/// One soft pool of light: where its centre sits as a fraction of the
/// window, its radii as fractions of the window's width and height, its
/// colour, and how strong it is at the centre.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pool {
    center: [f32; 2],
    radii: [f32; 2],
    color: Color32,
    strength: f32,
}

/// The pools behind the window, the lead one first, coloured by `lead`,
/// brightened or dimmed by `strength`.
fn pools(lead: Color32, strength: f32) -> [Pool; 3] {
    [
        Pool {
            center: [0.70, -0.10],
            radii: [0.80, 0.70],
            color: lead,
            strength: 0.44,
        },
        Pool {
            center: [0.02, 0.62],
            radii: [0.55, 0.65],
            color: VIOLET,
            strength: 0.36,
        },
        Pool {
            center: [0.98, 1.08],
            radii: [0.50, 0.45],
            color: BLUE,
            strength: 0.24,
        },
    ]
    .map(|pool| Pool {
        strength: (pool.strength * strength).min(1.0),
        ..pool
    })
}

/// A cover's colour made bright enough to glow: the album-art tint is
/// softened to sit behind text, so its hue is kept and its brightest
/// channel lifted. A grey cover gives the coral back.
pub fn vivid(color: Color32) -> Color32 {
    let [r, g, b, _] = color.to_srgba_unmultiplied();
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max == 0 || max - min < 12 {
        return CORAL;
    }
    let scale = 235.0 / f32::from(max);
    let lift = |channel: u8| (f32::from(channel) * scale).round().min(255.0) as u8;
    Color32::from_rgb(lift(r), lift(g), lift(b))
}

/// How bright a pool is at `t`, from its centre (0) to its rim (1): one
/// minus a smoothstep, so it has no visible edge at either end.
fn falloff(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

/// A pool as a mesh: a fan of rings, each a step dimmer than the one
/// inside it, ending transparent.
fn pool_mesh(rect: Rect, pool: Pool) -> Mesh {
    let mut mesh = Mesh::default();
    let center = pos2(
        rect.left() + rect.width() * pool.center[0],
        rect.top() + rect.height() * pool.center[1],
    );
    let radii = vec2(rect.width() * pool.radii[0], rect.height() * pool.radii[1]);
    let color = |t: f32| pool.color.gamma_multiply(pool.strength * falloff(t));
    mesh.colored_vertex(center, color(0.0));
    for ring in 1..=RINGS {
        let t = ring as f32 / RINGS as f32;
        let ring_color = color(t);
        for segment in 0..SEGMENTS {
            let angle = segment as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            let point = center + vec2(angle.cos() * radii.x, angle.sin() * radii.y) * t;
            mesh.colored_vertex(point, ring_color);
        }
    }
    let at = |ring: usize, segment: usize| -> u32 {
        (1 + (ring - 1) * SEGMENTS + segment % SEGMENTS) as u32
    };
    for segment in 0..SEGMENTS {
        mesh.add_triangle(0, at(1, segment), at(1, segment + 1));
    }
    for ring in 1..RINGS {
        for segment in 0..SEGMENTS {
            let (a, b) = (at(ring, segment), at(ring, segment + 1));
            let (c, d) = (at(ring + 1, segment), at(ring + 1, segment + 1));
            mesh.add_triangle(a, c, b);
            mesh.add_triangle(b, c, d);
        }
    }
    mesh
}

/// The base: a vertical gradient from a shade above the window to it.
fn base_mesh(rect: Rect, bottom: Color32) -> Mesh {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.left_top(), BASE_TOP);
    mesh.colored_vertex(rect.right_top(), BASE_TOP);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    mesh
}

/// The lead pool's colour, eased toward the cover's (or the coral).
fn eased_lead(ctx: &egui::Context, art: Option<Color32>) -> Color32 {
    let target = art.map_or(CORAL, vivid);
    let [r, g, b, _] = target.to_srgba_unmultiplied();
    let seconds = crate::motion::seconds(ctx, GLOW_FADE_SECONDS);
    let channel = |axis: &'static str, value: u8| {
        ctx.animate_value_with_time(
            egui::Id::new(("glass-lead", axis)),
            f32::from(value),
            seconds,
        )
        .round()
        .clamp(0.0, 255.0) as u8
    };
    Color32::from_rgb(channel("r", r), channel("g", g), channel("b", b))
}

/// Paints the backdrop under everything else in `ui`: call it before any
/// panel is shown. Nothing is painted for a solid palette.
pub fn backdrop(ui: &egui::Ui, palette: &Palette, art: Option<Color32>) {
    if !palette.glass {
        return;
    }
    let rect = ui.max_rect();
    let painter = ui.painter();
    painter.add(base_mesh(rect, palette.window));
    for pool in pools(eased_lead(ui.ctx(), art), palette.glass_strength) {
        painter.add(pool_mesh(rect, pool));
    }
}

/// Which side of a sheet meets the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// The hairline on the `side` of a sheet that meets the page. Nothing for
/// a solid palette, whose panels meet the page in a change of colour.
pub fn edge(ui: &egui::Ui, palette: &Palette, rect: Rect, side: Side) {
    if !palette.glass {
        return;
    }
    let x = match side {
        Side::Left => rect.left() + 0.5,
        Side::Right => rect.right() - 0.5,
    };
    let color = EDGE.gamma_multiply(sheet_scale(palette.glass_strength));
    ui.painter()
        .vline(x, rect.y_range(), Stroke::new(1.0, color));
}

/// The header's wash over glass: the cover's colour as light on the
/// sheet, fading out, rather than a solid band that would hide the
/// backdrop.
pub fn header_wash(art: Color32, overview: bool) -> (Color32, Color32) {
    let strength = if overview { 0.20 } else { 0.38 };
    (art.gamma_multiply(strength), Color32::TRANSPARENT)
}

/// The player bar's sheet at `strength` with the playing cover's colour
/// laid over it.
pub fn tinted_chrome(strength: f32, tint: Option<Color32>) -> Color32 {
    let chrome = chrome(strength);
    match tint {
        // Premultiplied colours add as one sheet laid over the other.
        Some(tint) => {
            let light = tint.gamma_multiply(0.18);
            Color32::from_rgba_premultiplied(
                chrome.r().saturating_add(light.r()),
                chrome.g().saturating_add(light.g()),
                chrome.b().saturating_add(light.b()),
                chrome.a().saturating_add(light.a()),
            )
        }
        None => chrome,
    }
}

/// The point at the centre of the window's lead pool, for tests.
#[cfg(test)]
fn lead_center(rect: Rect) -> egui::Pos2 {
    let pool = pools(CORAL, 1.0)[0];
    pos2(
        rect.left() + rect.width() * pool.center[0],
        rect.top() + rect.height() * pool.center[1],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Pos2;

    #[test]
    fn pools_fade_out_without_an_edge() {
        assert_eq!(falloff(0.0), 1.0);
        assert_eq!(falloff(1.0), 0.0);
        assert!(falloff(0.5) > 0.4 && falloff(0.5) < 0.6);
        let mut last = 1.0;
        for step in 1..=20 {
            let value = falloff(step as f32 / 20.0);
            assert!(value <= last);
            last = value;
        }
    }

    #[test]
    fn a_pool_ends_transparent_and_is_brightest_at_its_centre() {
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0));
        let mesh = pool_mesh(rect, pools(CORAL, 1.0)[0]);
        assert_eq!(mesh.vertices.len(), 1 + RINGS * SEGMENTS);
        assert_eq!(mesh.indices.len(), 3 * SEGMENTS * (1 + 2 * (RINGS - 1)));
        assert!(mesh.is_valid());
        assert_eq!(mesh.vertices[0].pos, lead_center(rect));
        let centre = mesh.vertices[0].color;
        let rim = mesh.vertices.last().unwrap().color;
        assert!(centre.a() > 0);
        assert_eq!(rim, Color32::TRANSPARENT);
    }

    #[test]
    fn a_cover_colour_glows_with_its_own_hue() {
        // The softened tint of a blue cover comes back bright and blue.
        let glow = vivid(Color32::from_rgb(0x20, 0x30, 0x60));
        assert_eq!(glow.b(), 235);
        assert!(glow.b() > glow.g() && glow.g() > glow.r());
        // A grey cover has no hue to give, so the coral stays.
        assert_eq!(vivid(Color32::from_rgb(0x50, 0x50, 0x52)), CORAL);
        assert_eq!(vivid(Color32::BLACK), CORAL);
    }

    #[test]
    fn the_header_wash_over_glass_is_light_not_a_band() {
        let art = Color32::from_rgb(0xc0, 0x40, 0x40);
        let (top, bottom) = header_wash(art, false);
        assert!(top.a() > 0 && top.a() < 255);
        assert_eq!(bottom, Color32::TRANSPARENT);
        assert!(header_wash(art, true).0.a() < top.a());
    }

    #[test]
    fn a_playing_cover_lights_the_bar_without_making_it_solid() {
        assert_eq!(tinted_chrome(1.0, None), CHROME);
        let tinted = tinted_chrome(1.0, Some(Color32::from_rgb(0x40, 0x80, 0xff)));
        assert!(tinted.a() > CHROME.a() && tinted.a() < 128);
        assert!(tinted.b() > tinted.r());
    }

    #[test]
    fn the_strength_brightens_the_light_and_the_sheets_by_half() {
        // The percentage stays inside what Settings offers.
        assert_eq!(strength(DEFAULT_PERCENT), 1.0);
        assert_eq!(strength(0), strength(MIN_PERCENT));
        assert_eq!(strength(255), strength(MAX_PERCENT));
        // At 100% everything is as designed.
        assert_eq!(chrome(1.0), CHROME);
        assert_eq!(sheet(20, 1.0), Color32::from_white_alpha(20));
        assert_eq!(pools(CORAL, 1.0)[0].strength, 0.44);
        // Stronger glass lifts the light fully and the sheets by half.
        let (weak, strong) = (strength(MIN_PERCENT), strength(MAX_PERCENT));
        for (low, high) in pools(CORAL, weak).iter().zip(pools(CORAL, strong)) {
            assert!(low.strength < high.strength && high.strength <= 1.0);
        }
        assert!(chrome(weak).a() < CHROME.a() && CHROME.a() < chrome(strong).a());
        assert!(sheet(20, weak).a() >= 10);
        assert!(sheet(44, strong).a() < 128);
    }
}
