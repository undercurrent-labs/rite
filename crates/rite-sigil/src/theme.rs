//! Themes: colour, weight, and glow, as versioned data rather than scattered
//! constants.
//!
//! A theme may change *how* a sigil looks and may never change *what* it says.
//! Every semantic distinction survives with colour removed (§4.6), which is why
//! `void` is a real theme rather than an accessibility afterthought — if a kind
//! stops being recognizable in monochrome, that is a bug in the mark, and `void`
//! is where it shows.
//!
//! Themes are typed Rust constants rather than a manifest file. `grammar/palette.json`
//! is a manifest because two independent implementations read it — a Rust
//! renderer and a TypeScript highlighter — and drift between them is invisible.
//! Sigil has one renderer (ADR 0005), so a manifest would add a parse, a
//! failure mode, and a file to keep in sync with nothing on the other side of it.
//!
//! The **theme version** is part of the render fingerprint. Changing a colour
//! changes the artifact, and a fingerprint that did not move would claim two
//! different pictures were the same one.

use serde::{Deserialize, Serialize};

use crate::graph::{CapabilityFamily, EdgeKind, SigilNodeKind};

/// Bumped whenever any theme's values change. Part of the render fingerprint.
pub const THEME_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeId {
    /// Dark void, cyan and magenta energy, gold seals. The default, and the most
    /// visually ambitious.
    #[default]
    NeonRitual,
    /// Near-monochrome. For print, engraving, iconography — and for proving that
    /// shape carries the meaning.
    Void,
    /// Warm manuscript, ink strokes, muted red and gold.
    Parchment,
}

impl ThemeId {
    pub fn name(self) -> &'static str {
        match self {
            ThemeId::NeonRitual => "neon-ritual",
            ThemeId::Void => "void",
            ThemeId::Parchment => "parchment",
        }
    }

    pub const ALL: &'static [ThemeId] = &[ThemeId::NeonRitual, ThemeId::Void, ThemeId::Parchment];

    pub fn parse(name: &str) -> Option<ThemeId> {
        ThemeId::ALL.iter().copied().find(|t| t.name() == name)
    }

    pub fn resolve(self) -> Theme {
        match self {
            ThemeId::NeonRitual => NEON_RITUAL,
            ThemeId::Void => VOID,
            ThemeId::Parchment => PARCHMENT,
        }
    }
}

/// One theme's values.
///
/// Every colour is an opaque `#rrggbb`. Opacity is separate, so a theme cannot
/// encode transparency in a colour string where the SVG writer would have to
/// parse it back out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub id: ThemeId,
    pub background: &'static str,
    /// Ordinary flow and stage strokes.
    pub stroke: &'static str,
    /// The entry, and anything the eye should reach first.
    pub core: &'static str,
    /// Gates, divergences — the constructs that change the shape of the stream.
    pub structure: &'static str,
    /// Seals, the output, the invocation boundary.
    pub seal: &'static str,
    /// Invocations.
    pub invocation: &'static str,
    /// Orbit rings and region geometry.
    pub region: &'static str,
    /// Unknown kinds and warnings.
    pub warning: &'static str,
    /// Inscriptions and the Codex, when drawn into the artifact.
    pub text: &'static str,
    /// Ornament, at low opacity.
    pub ornament: &'static str,

    pub node_stroke_width: f64,
    pub edge_stroke_width: f64,
    pub region_stroke_width: f64,
    /// Glow radius in user units. Zero disables the filter entirely, which is
    /// what makes `void` cheap to rasterize.
    pub glow: f64,
    pub ornament_opacity: f64,
    /// True when the theme is designed to be read without colour. `void` sets
    /// it, and the monochrome golden is generated from whichever themes do.
    pub monochrome: bool,
}

/// The default. Dark void, cyan and magenta semantic energy, gold seals.
pub const NEON_RITUAL: Theme = Theme {
    id: ThemeId::NeonRitual,
    background: "#05030A",
    stroke: "#38F2FF",
    core: "#EDEBFF",
    structure: "#FF3CCF",
    seal: "#D8B35C",
    invocation: "#8E5CFF",
    region: "#8E5CFF",
    warning: "#FF6B4A",
    text: "#EDEBFF",
    ornament: "#8E5CFF",
    node_stroke_width: 2.4,
    edge_stroke_width: 1.8,
    region_stroke_width: 1.4,
    glow: 3.0,
    ornament_opacity: 0.28,
    monochrome: false,
};

/// Monochrome. Where a mark that only works in colour goes to fail.
pub const VOID: Theme = Theme {
    id: ThemeId::Void,
    background: "#000000",
    stroke: "#F2F2F2",
    core: "#FFFFFF",
    structure: "#F2F2F2",
    seal: "#FFFFFF",
    invocation: "#F2F2F2",
    region: "#C8C8C8",
    warning: "#FFFFFF",
    text: "#FFFFFF",
    ornament: "#9A9A9A",
    node_stroke_width: 2.6,
    edge_stroke_width: 1.9,
    region_stroke_width: 1.5,
    glow: 0.0,
    ornament_opacity: 0.22,
    monochrome: true,
};

/// Occult manuscript: warm ground, ink strokes, muted red and gold.
pub const PARCHMENT: Theme = Theme {
    id: ThemeId::Parchment,
    background: "#EFE3C8",
    stroke: "#2B2118",
    core: "#1A130D",
    structure: "#8C2F1E",
    seal: "#8A6A22",
    invocation: "#5A4620",
    region: "#6B5230",
    warning: "#A33417",
    text: "#2B2118",
    node_stroke_width: 2.2,
    edge_stroke_width: 1.6,
    region_stroke_width: 1.3,
    glow: 0.0,
    ornament: "#6B5230",
    ornament_opacity: 0.3,
    monochrome: false,
};

impl Theme {
    /// The stroke for a node of a given kind.
    ///
    /// Colour groups kinds by *role* rather than giving each its own, because
    /// eleven distinguishable hues do not exist and pretending otherwise means
    /// the picture depends on telling two of them apart. The shape is the
    /// distinction; colour is the grouping.
    pub fn node_color(&self, kind: &SigilNodeKind) -> &'static str {
        match kind {
            SigilNodeKind::Source | SigilNodeKind::Literal => self.core,
            SigilNodeKind::Ward
            | SigilNodeKind::Scatter
            | SigilNodeKind::Fork
            | SigilNodeKind::Orbit => self.structure,
            SigilNodeKind::Collect | SigilNodeKind::Output => self.seal,
            SigilNodeKind::Effect => self.invocation,
            SigilNodeKind::Unknown(_) => self.warning,
            SigilNodeKind::Stage => self.stroke,
        }
    }

    pub fn edge_color(&self, kind: EdgeKind) -> &'static str {
        match kind {
            EdgeKind::Flow => self.stroke,
            EdgeKind::Enter | EdgeKind::Join => self.region,
            EdgeKind::Feedback => self.structure,
        }
    }

    /// An accent for a capability family.
    ///
    /// Supportive only. The family is already carried by the mark's topology
    /// (see `marks::effect`), so a viewer who cannot distinguish these loses
    /// nothing — which is the test `void` enforces by making them all identical.
    pub fn family_color(&self, family: &CapabilityFamily) -> &'static str {
        if self.monochrome {
            return self.invocation;
        }
        match family {
            CapabilityFamily::Fs => self.seal,
            CapabilityFamily::Net => self.stroke,
            CapabilityFamily::Db => self.invocation,
            CapabilityFamily::Console => self.core,
            CapabilityFamily::Clock => self.region,
            CapabilityFamily::Random => self.structure,
            CapabilityFamily::Env => self.seal,
            CapabilityFamily::Process => self.warning,
            CapabilityFamily::Mcp => self.stroke,
            CapabilityFamily::Other(_) => self.invocation,
        }
    }

    /// Dashes for an edge kind, in user units, or `None` for a solid stroke.
    ///
    /// Direction and role have to be readable without colour, so an entering
    /// trace and a returning one differ in *pattern* as well as in hue.
    pub fn edge_dash(&self, kind: EdgeKind) -> Option<&'static str> {
        match kind {
            EdgeKind::Flow => None,
            EdgeKind::Enter => Some("10 6"),
            EdgeKind::Join => Some("4 5"),
            EdgeKind::Feedback => Some("14 5 3 5"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_hex(color: &str) -> (f64, f64, f64) {
        let v = u32::from_str_radix(color.trim_start_matches('#'), 16).expect("hex colour");
        (
            ((v >> 16) & 0xff) as f64 / 255.0,
            ((v >> 8) & 0xff) as f64 / 255.0,
            (v & 0xff) as f64 / 255.0,
        )
    }

    /// WCAG relative luminance.
    fn luminance(color: &str) -> f64 {
        let (r, g, b) = parse_hex(color);
        let f = |c: f64| {
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
    }

    fn contrast(a: &str, b: &str) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn every_theme_id_round_trips_through_its_name() {
        for id in ThemeId::ALL {
            assert_eq!(ThemeId::parse(id.name()), Some(*id));
        }
        assert_eq!(ThemeId::parse("chartreuse"), None);
    }

    #[test]
    fn every_theme_resolves_to_itself() {
        for id in ThemeId::ALL {
            assert_eq!(id.resolve().id, *id);
        }
    }

    /// §14 and §23: a theme whose strokes cannot be read against its own
    /// background is a screenshot, not a renderer output. 3:1 is the WCAG
    /// non-text contrast floor, which is the right one for line art.
    #[test]
    fn every_semantic_stroke_is_readable_against_its_background() {
        for id in ThemeId::ALL {
            let theme = id.resolve();
            for (what, color) in [
                ("stroke", theme.stroke),
                ("core", theme.core),
                ("structure", theme.structure),
                ("seal", theme.seal),
                ("invocation", theme.invocation),
                ("region", theme.region),
                ("warning", theme.warning),
                ("text", theme.text),
            ] {
                let ratio = contrast(color, theme.background);
                assert!(
                    ratio >= 3.0,
                    "{}: {what} ({color}) is {ratio:.2}:1 against {} — below the 3:1 floor",
                    id.name(),
                    theme.background
                );
            }
        }
    }

    /// Colour is supportive, not authoritative. In `void` every family accent is
    /// the same, which is only survivable because the *mark* carries the family
    /// — and this test is what makes that dependency explicit.
    #[test]
    fn the_monochrome_theme_gives_every_family_the_same_accent() {
        let theme = ThemeId::Void.resolve();
        assert!(theme.monochrome);
        let first = theme.family_color(&CapabilityFamily::Fs);
        for family in CapabilityFamily::KNOWN {
            assert_eq!(theme.family_color(family), first);
        }
    }

    /// Edge roles differ in pattern as well as colour, so direction survives
    /// monochrome and colour-vision deficiency.
    #[test]
    fn every_edge_kind_has_its_own_dash_pattern() {
        let theme = ThemeId::NeonRitual.resolve();
        let patterns: Vec<Option<&str>> = [
            EdgeKind::Flow,
            EdgeKind::Enter,
            EdgeKind::Join,
            EdgeKind::Feedback,
        ]
        .iter()
        .map(|k| theme.edge_dash(*k))
        .collect();
        let mut unique = patterns.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), patterns.len(), "two edge kinds share a dash");
    }

    /// Every colour is an opaque six-digit hex. Opacity lives in its own field,
    /// so the SVG writer never has to parse a colour to find out.
    #[test]
    fn every_colour_is_an_opaque_six_digit_hex() {
        for id in ThemeId::ALL {
            let t = id.resolve();
            for color in [
                t.background,
                t.stroke,
                t.core,
                t.structure,
                t.seal,
                t.invocation,
                t.region,
                t.warning,
                t.text,
                t.ornament,
            ] {
                assert!(
                    color.len() == 7
                        && color.starts_with('#')
                        && color[1..].chars().all(|c| c.is_ascii_hexdigit()),
                    "{}: {color} is not #rrggbb",
                    id.name()
                );
            }
        }
    }

    /// The monochrome theme disables the glow filter outright rather than
    /// setting it small — a zero-radius blur still costs a filter pass in every
    /// rasterizer.
    #[test]
    fn the_monochrome_theme_has_no_glow() {
        assert_eq!(ThemeId::Void.resolve().glow, 0.0);
    }
}
