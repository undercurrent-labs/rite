//! Input limits.
//!
//! Sigil renders untrusted graphs — pasted into a web page, piped from a file,
//! written by a producer nobody audited. Every one of these caps exists because
//! the alternative is a browser tab that stops responding or a CLI that
//! allocates until the kernel intervenes.
//!
//! The defaults are the specification's §6.4 figures. They are all configurable
//! natively; the browser keeps the conservative ones, because a hung tab is a
//! worse failure than a refused render.

use serde::{Deserialize, Serialize};

/// What a render will accept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderLimits {
    /// Above this, a sigil stops being legible as an artifact. A warning, not a
    /// refusal: it is the user's picture and they may want it anyway.
    pub soft_node_warning: usize,
    /// Above this, refuse and say why. §11.8's position: a large graph is better
    /// viewed with `cant graph`, and quietly producing an unreadable circle
    /// serves nobody.
    pub max_nodes: usize,
    /// Edges are capped separately because a graph can be small in nodes and
    /// quadratic in edges, and it is edge routing that gets expensive.
    pub max_edges: usize,
    /// Region nesting depth. Bounded so containment analysis is a walk with a
    /// known bound rather than one that can be made to recurse arbitrarily.
    pub max_region_depth: usize,
    /// Bytes. A label longer than this is truncated, not rejected — losing the
    /// tail of one string is better than losing the render.
    pub max_label_bytes: usize,
    /// Bytes of serialized input. The first thing checked, before any parse,
    /// because everything after it is proportional to this.
    pub max_input_bytes: usize,
}

impl RenderLimits {
    /// Native defaults.
    pub const NATIVE: RenderLimits = RenderLimits {
        soft_node_warning: 250,
        max_nodes: 2_000,
        max_edges: 8_000,
        max_region_depth: 128,
        max_label_bytes: 4 * 1024,
        max_input_bytes: 16 * 1024 * 1024,
    };

    /// Browser defaults: the same shape, a smaller input ceiling.
    ///
    /// Only `max_input_bytes` differs. The node and edge caps are about whether
    /// a picture is legible, which is not a platform question; the input cap is
    /// about how much a tab should be asked to hold, which is.
    pub const BROWSER: RenderLimits = RenderLimits {
        max_input_bytes: 2 * 1024 * 1024,
        ..RenderLimits::NATIVE
    };
}

impl Default for RenderLimits {
    fn default() -> Self {
        RenderLimits::NATIVE
    }
}

/// How to normalize an incoming graph.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizeOptions {
    pub limits: RenderLimits,
    /// Keep source snippets in the normalized graph.
    ///
    /// Off by default. A snippet is the user's source, it is only needed for
    /// Revealed mode and the Codex, and a normalized graph that carries it by
    /// default is one that leaks it into every debug dump — the separation
    /// `docs/adr/0007-veil-and-source-privacy.md` insists on starts here, not at
    /// the SVG serializer.
    pub keep_snippets: bool,
    /// Reject a graph containing a node kind this renderer does not know,
    /// instead of drawing it with the unknown mark.
    ///
    /// Off by default, because §6.3 requires unknown kinds inside a supported
    /// schema version to degrade rather than fail. On for conformance tooling
    /// that wants to know when a producer has run ahead.
    pub strict_unknown_kinds: bool,
}

impl NormalizeOptions {
    pub fn browser() -> Self {
        NormalizeOptions {
            limits: RenderLimits::BROWSER,
            ..Default::default()
        }
    }

    pub fn with_snippets(mut self) -> Self {
        self.keep_snippets = true;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_browser_only_tightens_what_is_actually_a_platform_question() {
        // A `const` block, so the relationship is checked when the constants are
        // compiled rather than when someone runs the tests.
        const { assert!(RenderLimits::BROWSER.max_input_bytes < RenderLimits::NATIVE.max_input_bytes) };
        assert_eq!(
            RenderLimits::BROWSER.max_nodes,
            RenderLimits::NATIVE.max_nodes
        );
        assert_eq!(
            RenderLimits::BROWSER.max_edges,
            RenderLimits::NATIVE.max_edges
        );
        assert_eq!(
            RenderLimits::BROWSER.max_region_depth,
            RenderLimits::NATIVE.max_region_depth
        );
    }

    #[test]
    fn the_soft_warning_is_below_the_hard_cap() {
        let l = RenderLimits::NATIVE;
        assert!(
            l.soft_node_warning < l.max_nodes,
            "warning at {} is not below the cap at {}",
            l.soft_node_warning,
            l.max_nodes
        );
    }

    /// A snippet is the user's source. It arrives only when asked for.
    #[test]
    fn snippets_are_off_by_default() {
        assert!(!NormalizeOptions::default().keep_snippets);
        assert!(!NormalizeOptions::browser().keep_snippets);
        assert!(NormalizeOptions::default().with_snippets().keep_snippets);
    }

    /// §6.3: an unknown kind in a supported schema degrades, it does not fail.
    #[test]
    fn unknown_kinds_degrade_by_default() {
        assert!(!NormalizeOptions::default().strict_unknown_kinds);
    }
}
