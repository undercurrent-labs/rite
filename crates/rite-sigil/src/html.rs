//! Self-contained interactive HTML.
//!
//! One file, no network, no build step. It inlines the same SVG the canonical
//! export produces, adds a Codex built from the scene's legend, and wires hover,
//! focus and selection with application-managed listeners.
//!
//! # Why this is allowed to have a script when the SVG is not
//!
//! A standard SVG is a *picture*. It gets dropped into documents, opened by
//! image viewers, and embedded in pages that did not ask for behaviour, so a
//! script in one is a script somewhere nobody expected it (§22.2).
//!
//! An HTML export is an *application* the user asked for by name. §16.3 requires
//! tooltips and a collapsible Codex, which need behaviour. The script here is
//! ours, it is one block, it registers listeners rather than using inline
//! handlers, and it never evaluates anything derived from the graph — so a
//! `script-src 'sha256-…'` policy admits it and `unsafe-eval` is not needed.
//!
//! # Untrusted, all of it
//!
//! Every label, identifier and capability name is someone else's text. It goes
//! through [`escape_html`] on the way into markup and through
//! [`crate::svg::escape`] on the way into the SVG. Nothing is interpolated into
//! the script, ever: the Codex is built in the markup and the script only reads
//! `data-` attributes off elements the serializer wrote.

use std::fmt::Write as _;

use crate::scene::{SceneRef, SigilScene};
use crate::svg::{render_svg, sanitize_id, MetadataMode, SvgOptions};

/// How to build the interactive page.
#[derive(Debug, Clone, PartialEq)]
pub struct HtmlOptions {
    pub svg: SvgOptions,
    /// Show the Codex panel. Collapsed either way; this decides whether it is
    /// in the document at all.
    pub codex: bool,
    /// Embed the scene JSON in a `<script type="application/json">` block.
    ///
    /// Not executable — a non-JS `type` makes it inert data the browser will not
    /// run — and gated on the metadata mode, because a scene carries labels.
    pub embed_scene: bool,
}

impl Default for HtmlOptions {
    fn default() -> Self {
        HtmlOptions {
            svg: SvgOptions::default(),
            codex: true,
            embed_scene: false,
        }
    }
}

/// Render a self-contained interactive page.
pub fn render_html(scene: &SigilScene, options: &HtmlOptions) -> String {
    let rendered = render_svg(scene, &options.svg);
    let theme = options.svg.theme.resolve();
    let mut out = String::with_capacity(rendered.svg.len() * 2);

    out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n");
    out.push_str("<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n");
    let _ = writeln!(
        out,
        "<title>Sigil — {}</title>",
        escape_html(&scene.summary())
    );
    write_style(&mut out, theme.background, theme.text, theme.seal);
    out.push_str("</head>\n<body>\n");

    let _ = writeln!(
        out,
        "<main class=\"chamber\"><div class=\"canvas\">{}</div>",
        rendered.svg
    );

    if options.codex {
        write_codex(&mut out, scene, options.svg.metadata);
    }
    out.push_str("</main>\n");

    // A tooltip element the script positions, rather than one per node: a
    // document with one live region is one a screen reader can follow.
    out.push_str("<div id=\"sigil-tip\" role=\"status\" aria-live=\"polite\" hidden></div>\n");

    if options.embed_scene && options.svg.metadata == MetadataMode::Full {
        // `application/json` is inert: browsers do not execute it. The content
        // is still escaped, because `</script` inside a string would close the
        // element whatever its type says.
        if let Ok(json) = serde_json::to_string(scene) {
            let _ = writeln!(
                out,
                "<script type=\"application/json\" id=\"sigil-scene\">{}</script>",
                json.replace('<', "\\u003c")
            );
        }
    }

    write_script(&mut out);
    out.push_str("</body>\n</html>\n");
    out
}

fn write_style(out: &mut String, background: &str, text: &str, accent: &str) {
    // No user text reaches this; the only values are theme constants.
    let _ = write!(
        out,
        "<style>\
         :root{{color-scheme:dark}}\
         *{{box-sizing:border-box}}\
         body{{margin:0;background:{background};color:{text};\
         font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}}\
         .chamber{{display:flex;min-height:100vh;align-items:stretch}}\
         .canvas{{flex:1;display:flex;align-items:center;justify-content:center;padding:1rem;min-width:0}}\
         .canvas svg{{width:100%;height:auto;max-height:100vh}}\
         .codex{{width:22rem;max-width:40vw;border-left:1px solid {accent}40;\
         padding:1rem;overflow-y:auto;max-height:100vh}}\
         .codex[hidden]{{display:none}}\
         .codex h2{{font-size:.8rem;letter-spacing:.2em;text-transform:uppercase;opacity:.7;margin:0 0 .75rem}}\
         .entry{{padding:.5rem;border:1px solid transparent;border-radius:2px;cursor:pointer}}\
         .entry:hover,.entry:focus-visible,.entry[aria-current=true]{{border-color:{accent};outline:none}}\
         .entry .kind{{color:{accent};font-size:.75rem;letter-spacing:.1em;text-transform:uppercase}}\
         .entry .label{{display:block;opacity:.85;word-break:break-word;font-size:.8rem}}\
         .entry dl{{margin:.25rem 0 0;font-size:.7rem;opacity:.6}}\
         .entry dt{{display:inline;font-weight:400}}\
         .entry dd{{display:inline;margin:0 .5rem 0 .25rem}}\
         .toggle{{position:fixed;top:.75rem;right:.75rem;background:transparent;\
         color:{text};border:1px solid {accent};border-radius:2px;padding:.4rem .7rem;\
         font:inherit;font-size:.75rem;cursor:pointer}}\
         #sigil-tip{{position:fixed;pointer-events:none;padding:.3rem .5rem;\
         background:{background};border:1px solid {accent};border-radius:2px;\
         font-size:.75rem;max-width:20rem;z-index:10}}\
         #sigil-tip[hidden]{{display:none}}\
         [data-sigil-node]{{cursor:pointer}}\
         [data-sigil-node]:focus{{outline:2px solid {accent};outline-offset:2px}}\
         .dimmed{{opacity:.15;transition:opacity .12s}}\
         @media (prefers-reduced-motion:reduce){{*{{transition:none!important;animation:none!important}}}}\
         @media (max-width:52rem){{.chamber{{flex-direction:column}}\
         .codex{{width:100%;max-width:none;border-left:0;border-top:1px solid {accent}40;max-height:45vh}}}}\
         </style>"
    );
    out.push('\n');
}

fn write_codex(out: &mut String, scene: &SigilScene, metadata: MetadataMode) {
    out.push_str(
        "<button class=\"toggle\" id=\"sigil-codex-toggle\" aria-expanded=\"false\" \
         aria-controls=\"sigil-codex\">Codex</button>\n",
    );
    // Collapsed by default (§13.4's web default), and `hidden` rather than
    // display-none-by-class so assistive technology agrees with the pixels.
    out.push_str("<aside class=\"codex\" id=\"sigil-codex\" hidden>\n");
    let _ = writeln!(
        out,
        "<h2>Codex</h2>\n<p class=\"summary\">{}</p>",
        escape_html(&scene.summary())
    );

    for entry in &scene.legend {
        let SceneRef::Node(id) = &entry.graph_ref else {
            continue;
        };
        let _ = write!(
            out,
            "<div class=\"entry\" tabindex=\"0\" data-for=\"node-{}\">\
             <span class=\"kind\">{}</span>",
            sanitize_id(id),
            escape_html(&entry.summary)
        );
        // A label only exists when the graph carried one, which only happens
        // when labels were asked for — and never under `metadata none`.
        if metadata != MetadataMode::None {
            if let Some(label) = &entry.label {
                let _ = write!(out, "<span class=\"label\">{}</span>", escape_html(label));
            }
        }
        // Gated on metadata for the same reason the label is. A legend entry's
        // capability list carries the *name* — `@fs.read` — whenever the graph
        // carried one, which is user text; the first version of this wrote it
        // unconditionally, so `--metadata none` kept it out of the label and let
        // it back in one line lower. The family name in the entry's summary is
        // always safe and is what remains.
        if metadata != MetadataMode::None && !entry.capabilities.is_empty() {
            let _ = write!(
                out,
                "<dl><dt>touches</dt><dd>{}</dd></dl>",
                escape_html(&entry.capabilities.join(", "))
            );
        }
        if metadata == MetadataMode::Full {
            if let Some(span) = entry.source_span {
                let _ = write!(
                    out,
                    "<dl><dt>span</dt><dd>{}..{}</dd></dl>",
                    span.start.as_usize(),
                    span.end.as_usize()
                );
            }
        }
        for warning in &entry.warnings {
            let _ = write!(out, "<dl><dt>!</dt><dd>{}</dd></dl>", escape_html(warning));
        }
        out.push_str("</div>\n");
    }
    out.push_str("</aside>\n");
}

/// The behaviour. One block, listeners only, nothing interpolated.
///
/// It reads `<title>` off the hovered element for the tooltip — the same text
/// the accessible name uses, which is a semantic kind and never a label, so the
/// tooltip cannot show more than the disclosure mode allowed.
fn write_script(out: &mut String) {
    out.push_str(
        r#"<script>
(function () {
  var svg = document.querySelector('.canvas svg');
  if (!svg) return;
  var tip = document.getElementById('sigil-tip');
  var codex = document.getElementById('sigil-codex');
  var toggle = document.getElementById('sigil-codex-toggle');
  var nodes = svg.querySelectorAll('[id^="node-"]');

  nodes.forEach(function (node) {
    node.setAttribute('tabindex', '0');
    node.setAttribute('data-sigil-node', '');
    var title = node.querySelector('title');
    var text = title ? title.textContent : '';
    function show(event) {
      if (!text) return;
      tip.textContent = text;
      tip.hidden = false;
      var box = node.getBoundingClientRect();
      tip.style.left = Math.round(box.left + box.width / 2) + 'px';
      tip.style.top = Math.round(box.top - 8) + 'px';
    }
    function hide() { tip.hidden = true; }
    node.addEventListener('mouseenter', show);
    node.addEventListener('focus', show);
    node.addEventListener('mouseleave', hide);
    node.addEventListener('blur', hide);
    node.addEventListener('click', function () { select(node.id); });
    node.addEventListener('keydown', function (e) {
      if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); select(node.id); }
    });
  });

  var selected = null;
  function select(id) {
    selected = selected === id ? null : id;
    nodes.forEach(function (n) {
      n.classList.toggle('dimmed', selected !== null && n.id !== selected);
    });
    document.querySelectorAll('.entry').forEach(function (entry) {
      var mine = entry.getAttribute('data-for') === selected;
      entry.setAttribute('aria-current', mine ? 'true' : 'false');
      if (mine) entry.scrollIntoView({ block: 'nearest' });
    });
  }

  document.querySelectorAll('.entry').forEach(function (entry) {
    function pick() { select(entry.getAttribute('data-for')); }
    entry.addEventListener('click', pick);
    entry.addEventListener('keydown', function (e) {
      if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); pick(); }
    });
  });

  if (toggle && codex) {
    toggle.addEventListener('click', function () {
      var open = codex.hidden;
      codex.hidden = !open;
      toggle.setAttribute('aria-expanded', open ? 'true' : 'false');
    });
  }

  document.addEventListener('keydown', function (e) {
    if (e.key !== 'Escape') return;
    tip.hidden = true;
    if (selected) select(selected);
    if (codex && !codex.hidden && toggle) toggle.click();
  });
})();
</script>
"#,
    );
}

/// The only way text reaches the HTML.
///
/// Separate from [`crate::svg::escape`] because the contexts differ: HTML has
/// raw-text elements — `<script>`, `<style>` — where `&lt;` is not an escape,
/// so `</` sequences have to be broken rather than entity-encoded. Nothing user-
/// supplied is written into either of those here, and this function is what keeps
/// that true if something ever is.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_escaping_covers_every_entity() {
        assert_eq!(
            escape_html(r#"<a href="x">&'"#),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;"
        );
    }

    #[test]
    fn control_characters_are_dropped() {
        assert_eq!(escape_html("a\u{0}b\u{1}c"), "abc");
    }

    #[test]
    fn the_defaults_show_a_codex_and_embed_no_scene() {
        let options = HtmlOptions::default();
        assert!(options.codex);
        assert!(!options.embed_scene, "a scene carries labels");
    }
}
