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
        // The one configuration a workspace build never covers.
        //
        // `cargo check --workspace` and `cargo test --all-features` both enable
        // every feature, so a `resvg` call that lost its `#[cfg(feature =
        // "png")]` compiled locally and broke only in CI's WASM job, which takes
        // `rite-render` with default features off. This is that build, minus the
        // wasm-pack packaging.
        "wasm-check" => {
            run(Command::new("rustup").args(["target", "add", "wasm32-unknown-unknown"]));
            // Both browser crates. `cant-wasm` is the same trap with a second
            // floor: it must not pull Rite's capability stack, and cargo ignores
            // `default-features = false` on a workspace dependency, so the
            // wrong-but-plausible manifest compiles everywhere except here.
            for package in ["rite-wasm", "cant-wasm"] {
                run(Command::new("cargo").args([
                    "check",
                    "-p",
                    package,
                    "--no-default-features",
                    "--features",
                    "wasm",
                    "--target",
                    "wasm32-unknown-unknown",
                ]));
            }
            // The browser capability host, run rather than only compiled.
            //
            // `cargo check` for wasm32 proves it builds; it cannot prove
            // `@json.encode` answers or that `@fs.read` is refused by name.
            // Without `native`, `rite-wasm` builds for the host too, so the
            // same code the browser runs is testable here in seconds.
            run(Command::new("cargo").args(["test", "-p", "rite-wasm", "--no-default-features"]));
        }
        "sigil-og" => {
            if let Err(e) = sigil_og() {
                eprintln!("sigil-og: {e:#}");
                std::process::exit(1);
            }
        }
        "cant-og" => {
            if let Err(e) = cant_og() {
                eprintln!("cant-og: {e:#}");
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!(
                "xtask commands: test | fmt | clippy | doc | examples | wasm-check | cant-og | sigil-og"
            );
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
/// The Sigil site's social card: family ground, violet accent, and the
/// ceremony example's actual render — the golden from `fixtures/sigil/svg`,
/// inlined as vector art so the card shows the product, not a screenshot.
/// Removed with Cant, like `cant-og`: the fixture it reads goes with the
/// language that produces it.
fn sigil_og() -> anyhow::Result<()> {
    const WIDTH: u32 = 1200;
    const HEIGHT: u32 = 630;

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow::anyhow!("no parent of xtask/"))?
        .to_path_buf();

    let ceremony = std::fs::read_to_string(root.join("fixtures/sigil/svg/ceremony.veiled.svg"))?;
    // Re-window the golden into the card's right-hand panel. The golden's own
    // opening tag carries the 1600-square viewBox; everything after it is the
    // artwork, background included.
    let body = ceremony
        .split_once('>')
        .map(|(_, rest)| rest)
        .ok_or_else(|| anyhow::anyhow!("the ceremony golden has no opening tag"))?;
    // Drop the golden's own background rect: the artwork floats on the card's
    // ground instead of arriving in an opaque panel that crops the headline.
    let body = match body.find("<rect") {
        Some(start) => match body[start..].find("/>") {
            Some(rel_end) => {
                let end = start + rel_end + 2;
                format!("{}{}", &body[..start], &body[end..])
            }
            None => body.to_string(),
        },
        None => body.to_string(),
    };
    let inset =
        format!(r##"<svg x="560" y="10" width="630" height="630" viewBox="0 0 1600 1600">{body}"##);

    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}" viewBox="0 0 {WIDTH} {HEIGHT}">
  <rect width="{WIDTH}" height="{HEIGHT}" fill="#0b0f14"/>
  <rect x="0" y="0" width="{WIDTH}" height="6" fill="#c792ea"/>

  <rect x="88" y="88" width="92" height="92" rx="18" fill="#05030A" stroke="#c792ea" stroke-opacity="0.35" stroke-width="2"/>
  <g fill="none" transform="translate(88 88) scale(0.71875)">
    <circle cx="64" cy="64" r="44" stroke="#8E5CFF" stroke-width="3" opacity="0.7"/>
    <path d="M64 30 L64 52 M31 83 L53 70.5 M97 83 L75 70.5" stroke="#FF3CCF" stroke-width="4" stroke-linecap="round"/>
    <path d="M64 11 L73 20 L64 29 L55 20 Z" stroke="#38F2FF" stroke-width="4"/>
    <circle cx="26" cy="86" r="7" stroke="#38F2FF" stroke-width="4"/>
    <circle cx="102" cy="86" r="7" stroke="#38F2FF" stroke-width="4"/>
    <circle cx="64" cy="64" r="12" stroke="#D8B35C" stroke-width="4"/>
    <circle cx="64" cy="64" r="3.5" fill="#D8B35C" stroke="none"/>
  </g>

  <text x="208" y="152" font-family="DejaVu Sans Mono, monospace" font-size="52" font-weight="700" fill="#c792ea">Sigil</text>

  <text x="88" y="290" font-family="DejaVu Sans, sans-serif" font-size="46" font-weight="700" fill="#e2e8f0">A program's topology</text>
  <text x="88" y="352" font-family="DejaVu Sans, sans-serif" font-size="46" font-weight="700" fill="#e2e8f0">as a <tspan fill="#c792ea">ritual artifact</tspan>.</text>

  <text x="88" y="440" font-family="DejaVu Sans, sans-serif" font-size="24" fill="#8b9bb4">Deterministic. Veiled by default.</text>
  <text x="88" y="478" font-family="DejaVu Sans, sans-serif" font-size="24" fill="#8b9bb4">Rendered in your browser — never uploaded.</text>

  <text x="88" y="562" font-family="DejaVu Sans Mono, monospace" font-size="25" fill="#c792ea">sigil.rite.foo</text>

  {inset}
</svg>"##
    );

    let png = rite_render::svg_to_png(&svg, 1.0)?;
    let target = root.join("apps/sigil-web/public/og.png");
    std::fs::create_dir_all(target.parent().expect("parent"))?;
    std::fs::write(&target, &png)?;
    println!(
        "wrote {} ({} bytes, {WIDTH}x{HEIGHT})",
        target.strip_prefix(&root).unwrap_or(&target).display(),
        png.len()
    );
    Ok(())
}

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
