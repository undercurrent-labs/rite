//! Interactive Rite REPL.

use rite_caps::{install_defaults, Permission, PermissionSet};
use rite_core::SourceFile;
use rite_runtime::{run_file, RuntimeContext, Value};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

pub async fn run_repl(perms: PermissionSet) -> anyhow::Result<()> {
    println!("Rite {} — type :help for commands", env!("CARGO_PKG_VERSION"));
    let mut rl = DefaultEditor::new()?;
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, perms.clone());
    let mut perms = perms;
    let mut buffer = String::new();
    let mut glyph = true;

    loop {
        let prompt = if buffer.is_empty() {
            "rite〉"
        } else {
            "   ·〉"
        };
        let line = match rl.readline(prompt) {
            Ok(l) => l,
            Err(ReadlineError::Interrupted) => {
                if buffer.is_empty() {
                    println!("^C");
                    continue;
                } else {
                    buffer.clear();
                    continue;
                }
            }
            Err(ReadlineError::Eof) => {
                println!("bye");
                break;
            }
            Err(e) => return Err(e.into()),
        };

        let trimmed = line.trim();
        if buffer.is_empty() && trimmed.starts_with(':') {
            if handle_meta(trimmed, &mut ctx, &mut perms, &mut glyph).await? {
                break;
            }
            continue;
        }

        buffer.push_str(&line);
        buffer.push('\n');

        // Heuristic: complete when braces/blocks balanced
        if !is_complete(&buffer) {
            continue;
        }

        let src = std::mem::take(&mut buffer);
        let _ = rl.add_history_entry(src.trim());
        let sf = SourceFile::new(rite_core::FileId(0), "<repl>".to_string(), src);
        match run_file(&sf, &mut ctx).await {
            Ok(Value::None) => {}
            Ok(v) => println!("{}", v.to_display(&ctx.atoms)),
            Err(e) => eprintln!("error: {}", e),
        }
    }
    Ok(())
}

async fn handle_meta(
    cmd: &str,
    ctx: &mut RuntimeContext,
    perms: &mut PermissionSet,
    glyph: &mut bool,
) -> anyhow::Result<bool> {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    match parts.first().copied().unwrap_or("") {
        ":help" => {
            println!(
                r#":help              Show this help
:load path         Load a file
:reload            Reload last file (not persisted in v1)
:bindings          Show bindings
:modules           Show modules
:capabilities      List capabilities
:allow perm        Grant permission
:deny perm         Deny permission
:format glyph|ascii
:reset             Reset environment
:quit / :q         Exit"#
            );
        }
        ":quit" | ":q" => return Ok(true),
        ":bindings" => {
            for (k, v) in ctx.env.bindings_snapshot() {
                println!("{} = {}", k, v.to_display(&ctx.atoms));
            }
        }
        ":capabilities" => {
            println!("console fs json clock env process random http game store");
        }
        ":allow" => {
            if let Some(spec) = parts.get(1) {
                if let Ok(p) = Permission::parse(spec) {
                    perms.grant(p);
                    install_defaults(ctx, perms.clone());
                    println!("allowed {}", spec);
                }
            }
        }
        ":deny" => {
            if let Some(spec) = parts.get(1) {
                if let Ok(p) = Permission::parse(spec) {
                    perms.deny(p);
                    install_defaults(ctx, perms.clone());
                    println!("denied {}", spec);
                }
            }
        }
        ":format" => {
            match parts.get(1).copied() {
                Some("ascii") => *glyph = false,
                Some("glyph") => *glyph = true,
                _ => println!("usage: :format glyph|ascii"),
            }
            println!("format: {}", if *glyph { "glyph" } else { "ascii" });
        }
        ":reset" => {
            *ctx = RuntimeContext::new();
            install_defaults(ctx, perms.clone());
            println!("reset");
        }
        ":load" => {
            if let Some(path) = parts.get(1) {
                let text = std::fs::read_to_string(path)?;
                let sf = SourceFile::new(rite_core::FileId(0), (*path).to_string(), text);
                match run_file(&sf, ctx).await {
                    Ok(v) => {
                        if !matches!(v, Value::None) {
                            println!("{}", v.to_display(&ctx.atoms));
                        }
                    }
                    Err(e) => eprintln!("error: {}", e),
                }
            }
        }
        ":modules" => println!("(main)"),
        other => println!("unknown command {}", other),
    }
    Ok(false)
}

fn is_complete(src: &str) -> bool {
    let mut depth = 0i32;
    let mut in_str = false;
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        if in_str {
            if c == '\\' {
                let _ = chars.next();
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '⟦' | '[' | '{' | '(' | '⟨' => depth += 1,
            '⟧' | ']' | '}' | ')' | '⟩' => depth -= 1,
            _ => {}
        }
        // ASCII blocks [[ ]]
    }
    // Also count [[ and ]]
    let open = src.matches("[[").count() + src.matches("<<").count();
    let close = src.matches("]]").count() + src.matches(">>").count();
    depth + (open as i32 - close as i32) <= 0 && !src.trim().is_empty()
}
