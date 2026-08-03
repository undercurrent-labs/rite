//! Dev task runner for Rite.

use std::process::Command;

fn main() {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_else(|| "help".into());
    match cmd.as_str() {
        "test" => {
            run(Command::new("cargo").args(["test", "--workspace"]));
        }
        "fmt" => {
            run(Command::new("cargo").args(["fmt", "--all"]));
        }
        "clippy" => {
            run(Command::new("cargo").args([
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ]));
        }
        "doc" => {
            run(Command::new("cargo").args(["run", "-p", "rite-cli", "--", "doc"]));
        }
        "examples" => {
            for ex in [
                "examples/hello/hello.rite",
                "examples/automation/logs.rite",
                "examples/data-pipeline/summarize.rite",
                "examples/text-rpg/game.rite",
                "examples/modules/main.rite",
            ] {
                println!("==> {ex}");
                run(Command::new("cargo").args([
                    "run",
                    "-p",
                    "rite-cli",
                    "--",
                    "run",
                    ex,
                    "--allow-all",
                ]));
            }
        }
        "cant-og" => {
            if let Err(e) = cant_og() {
                eprintln!("cant-og: {e:#}");
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("xtask commands: test | fmt | clippy | doc | examples | cant-og");
        }
    }
}

/// Build the social card for `cant.rite.foo`.
///
/// A hand-authored SVG rasterised through `rite_render::svg_to_png`. Scrapers do
/// not render SVG — Twitter, Slack and iMessage all want a raster — so this has
/// to produce a PNG, and it does it with the rasteriser already in this tree
/// rather than an image toolchain someone has to install.
///
/// Regenerate after changing the card or Rite's logo:
///
///     cargo run -p xtask -- cant-og
fn cant_og() -> anyhow::Result<()> {
    const WIDTH: u32 = 1200;
    const HEIGHT: u32 = 630;

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow::anyhow!("no parent of xtask/"))?
        .to_path_buf();

    // The mark is drawn, not embedded.
    //
    // `apps/cant-web/public/brand/logo.svg` carries Rite's PNG as a data URI,
    // and the rasteriser drops it: `rite-render` builds `resvg` without
    // `raster-images` on purpose, to keep a highlighter from pulling GIF, WebP
    // and JPEG decoders it will never use. Turning that on for one 96px monogram
    // is the wrong trade, so the card draws the part that carries the meaning —
    // the pink tilde, in a panel — and lets the wordmark say the rest.
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}" viewBox="0 0 {WIDTH} {HEIGHT}">
  <rect width="{WIDTH}" height="{HEIGHT}" fill="#0b0f14"/>
  <rect x="0" y="0" width="{WIDTH}" height="6" fill="#ff7edb"/>

  <rect x="88" y="88" width="92" height="92" rx="18" fill="#121821" stroke="#ff7edb" stroke-opacity="0.35" stroke-width="2"/>
  <path d="M 108 146 C 118 122, 136 124, 144 137 C 151 150, 168 152, 176 126"
        fill="none" stroke="#ff7edb" stroke-width="11" stroke-linecap="round"/>

  <text x="208" y="152" font-family="DejaVu Sans Mono, monospace" font-size="48" font-weight="700">
    <tspan fill="#64748b" text-decoration="line-through">Rite</tspan><tspan fill="#ff7edb" dx="20">-&gt;</tspan><tspan fill="#ff7edb" dx="20">Cant</tspan>
  </text>

  <text x="88" y="278" font-family="DejaVu Sans, sans-serif" font-size="54" font-weight="700" fill="#e2e8f0">A graph-oriented language</text>
  <text x="88" y="348" font-family="DejaVu Sans, sans-serif" font-size="54" font-weight="700" fill="#e2e8f0">you can <tspan fill="#ff7edb">type into a terminal</tspan>.</text>

  <rect x="88" y="402" width="1024" height="100" rx="12" fill="#121821" stroke="#1e293b"/>
  <text x="120" y="462" font-family="DejaVu Sans Mono, monospace" font-size="29">
    <tspan fill="#e2e8f0">[1, 2, 3, 4, 5, 6] </tspan><tspan fill="#ff7edb">-&gt; * -&gt; ?{{</tspan><tspan fill="#89ddff"> $ </tspan><tspan fill="#e2e8f0">% 2 = 0 </tspan><tspan fill="#ff7edb">}} -&gt; []</tspan>
  </text>

  <text x="88" y="562" font-family="DejaVu Sans, sans-serif" font-size="25" fill="#8b9bb4">A sibling to Rite. Lowers to canonical Rite; runs on Rite's runtime.</text>
  <text x="1112" y="562" text-anchor="end" font-family="DejaVu Sans Mono, monospace" font-size="25" fill="#7ee0ff">cant.rite.foo</text>
</svg>"##
    );

    let png = rite_render::svg_to_png(&svg, 1.0)?;
    let target = root.join("apps/cant-web/public/og.png");
    std::fs::create_dir_all(target.parent().expect("parent"))?;
    std::fs::write(&target, &png)?;
    println!(
        "wrote {} ({} bytes, {WIDTH}x{HEIGHT})",
        target.strip_prefix(&root).unwrap_or(&target).display(),
        png.len()
    );
    Ok(())
}

fn run(cmd: &mut Command) {
    let status = cmd.status().expect("spawn");
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
}
