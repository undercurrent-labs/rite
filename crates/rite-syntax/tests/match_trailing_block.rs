//! Match scrutinee must not swallow the arms block as a trailing call.

use rite_syntax::parse_source;

#[test]
fn match_with_block_arms() {
    let src = r#"
status ← #ok
msg ← ~ status ⟦
  #ok → "ready"
  _ → "unknown"
⟧
msg
"#;
    let (p, d, _) = parse_source("m.rite", src);
    assert!(!d.has_errors(), "{:?}", d.into_vec());
    assert!(p.is_some());
}

#[test]
fn if_with_block_not_call() {
    let src = r#"
x ← 1
? x ⟦
  #yes
⟧ : ⟦
  #no
⟧
"#;
    let (p, d, _) = parse_source("i.rite", src);
    assert!(!d.has_errors(), "{:?}", d.into_vec());
    assert!(p.is_some());
}

#[test]
fn keep_trailing_block_still_works() {
    let src = "xs → keep { |x| x }\n";
    let (p, d, _) = parse_source("k.rite", src);
    assert!(!d.has_errors(), "{:?}", d.into_vec());
    assert!(p.is_some());
}
