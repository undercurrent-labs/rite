use rite_fmt::format_source;

#[test]
fn format_idempotent_glyph() {
    let src = "x ← 1 + 2\n";
    let once = format_source(src, false).unwrap();
    let twice = format_source(&once, false).unwrap();
    assert_eq!(once, twice);
}

#[test]
fn format_ascii_roundtrip_parse() {
    let src = "def f(x) [[ return x * 2 ]]\n";
    let glyph = format_source(src, false).unwrap();
    let ascii = format_source(&glyph, true).unwrap();
    assert!(
        ascii.contains("def")
            || ascii.contains("<-")
            || ascii.contains("return")
            || ascii.contains("[[")
    );
    let again = format_source(&ascii, true).unwrap();
    assert_eq!(ascii, again);
}
