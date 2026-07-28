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
        _ => {
            eprintln!("xtask commands: test | fmt | clippy | doc | examples");
        }
    }
}

fn run(cmd: &mut Command) {
    let status = cmd.status().expect("spawn");
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
}
