//! Compile-fail tests.
//!
//! `Strategy` is an unsafe trait, which is a refusal rather than a value, so nothing in the
//! ordinary suite can reach it: a test asserting that a safe impl does not compile cannot
//! itself compile. trybuild builds each case as its own crate and asserts the diagnostic, so
//! the refusal is pinned and a later loosening of the bound fails here instead of silently
//! restoring the hole.

/// Every case in `tests/ui`, so a file added without a line here fails rather than going
/// unrun. The counts are the whole point: an empty glob passes.
#[test]
fn the_ui_cases_hold() {
    const EXPECTED_FAILING: usize = 1;
    const EXPECTED_PASSING: usize = 1;

    let failing = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ui"))
        .expect("tests/ui")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "rs"))
        .count();
    let passing = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ui/pass"))
        .expect("tests/ui/pass")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "rs"))
        .count();

    assert_eq!(
        failing, EXPECTED_FAILING,
        "a compile-fail case was added or removed without updating this count, so trybuild's \
         glob would have run a different set than the one this test claims",
    );
    assert_eq!(
        passing, EXPECTED_PASSING,
        "a compile-pass case was added or removed without updating this count",
    );

    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
    t.pass("tests/ui/pass/*.rs");
}
