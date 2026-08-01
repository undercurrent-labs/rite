//! Converting a dialect must not change what a program computes.
//!
//! `rite fmt` rewrites source between the glyph and ASCII spellings, and the
//! formatter's own tests assert on the *text* it produces. Text is the weaker
//! property: `÷` was printed as the word `idiv` in ASCII on the strength of an alias
//! table entry, and `idiv` does not lex as an operator — both it and `compose` are
//! taken by the builtins they lower to, so a keyword would collide with `idiv(7, 2)`.
//!
//! The output parsed, so nothing complained, and it meant something else: `7 ÷ 2` is
//! 3, and the ASCII rendering `7 idiv 2` is two statements evaluating to 7. `f ∘ g`
//! became `f compose g`, which is `f`. A formatter that changes the answer is worse
//! than one that refuses.
//!
//! This runs both spellings and compares the values.

use rite_runtime::RuntimeContext;

async fn value_of(src: &str) -> String {
    let mut ctx = RuntimeContext::new();
    match rite_runtime::run_source("t.rite", src, &mut ctx).await {
        Ok(v) => format!("{v}"),
        Err(e) => format!("error: {e}"),
    }
}

#[tokio::test]
async fn formatting_between_dialects_preserves_the_value() {
    for src in [
        "^ 7 ÷ 2\n",
        "◆ inc(n) ⟦ ^ n + 1 ⟧\n◆ dbl(n) ⟦ ^ n * 2 ⟧\nc ← dbl ∘ inc\n^ c(5)\n",
        "^ 2 ** 8\n",
        "^ count(1 ‥ 5)\n",
        "^ count(1 .. 5)\n",
        "^ [1, 2, 3, 4] → keep ⟦ |n| n % 2 = 0 ⟧ → sum\n",
    ] {
        let before = value_of(src).await;
        for to_ascii in [true, false] {
            let formatted = rite_fmt::format_source(src, to_ascii).expect("format");
            let after = value_of(&formatted).await;
            assert_eq!(
                before,
                after,
                "formatting changed the value (ascii={to_ascii})\n--- before ---\n{src}--- after ---\n{formatted}"
            );
        }
    }
}
