//! The README's feature table names the features the manifest has, and no
//! others.
//!
//! A feature table is a claim about the manifest, and this one had drifted both
//! ways at once: it listed a `std` feature that does not exist, and said `tlsf`
//! "gates nothing today", which stopped being true when the flag was made real.
//! Neither breaks a build, so nothing was going to notice.

use std::collections::BTreeSet;
use std::fs;

/// The README row for one feature, or `None` when the table has no row for it.
fn feature_row<'a>(readme: &'a str, name: &str) -> Option<&'a str> {
    readme
        .lines()
        .find(|line| line.starts_with(&format!("| `{}`", name)))
}

/// The feature names the manifest declares, excluding `default`.
///
/// Parsed rather than hardcoded, so adding a feature to the manifest and
/// forgetting the README is what fails rather than adding one to both and
/// forgetting this.
fn manifest_features() -> BTreeSet<String> {
    let manifest = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("the manifest");

    let section = manifest
        .split("\n[features]\n")
        .nth(1)
        .expect("a features section")
        .split("\n[")
        .next()
        .expect("the section body");

    section
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split('=').next())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty() && name != "default")
        .collect()
}

/// The feature names the README's table has in its first column.
fn readme_features() -> BTreeSet<String> {
    let readme =
        fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md")).expect("the readme");

    readme
        .lines()
        .filter(|line| line.starts_with("| `"))
        .filter_map(|line| line.split('`').nth(1))
        .map(str::to_string)
        .collect()
}

#[test]
fn the_readme_documents_exactly_the_features_that_exist() {
    let manifest = manifest_features();
    let readme = readme_features();

    assert!(
        !manifest.is_empty(),
        "the manifest parse found no features, so this checks nothing"
    );

    let undocumented: Vec<&String> = manifest.difference(&readme).collect();
    assert!(
        undocumented.is_empty(),
        "the manifest has features the README does not document: {:?}",
        undocumented,
    );

    let invented: Vec<&String> = readme.difference(&manifest).collect();
    assert!(
        invented.is_empty(),
        "the README documents features the manifest does not have: {:?}",
        invented,
    );
}

#[test]
fn the_readme_says_which_features_are_on_by_default() {
    // The other half of the table, and the half that goes stale silently: a feature
    // moving in or out of the default set changes what a consumer gets without
    // changing any name.
    let manifest = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("the manifest");

    let defaults: Vec<&str> = manifest
        .lines()
        .find(|line| line.trim_start().starts_with("default = ["))
        .and_then(|line| line.split('[').nth(1))
        .and_then(|rest| rest.split(']').next())
        .expect("a default list")
        .split(',')
        .map(|name| name.trim().trim_matches('"'))
        .filter(|name| !name.is_empty())
        .collect();

    let readme =
        fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md")).expect("the readme");

    // Both directions, because either one alone passes while the table is wrong.
    // Checking only that every default says `yes` lets the README mark a
    // feature `yes` that nobody gets; checking only the reverse lets a new
    // default go undocumented. An earlier version of this test asserted the
    // default set had two entries, which is neither direction: it compared the
    // manifest against a literal in this file and never read the README at all.
    for name in &defaults {
        let row = feature_row(&readme, name)
            .unwrap_or_else(|| panic!("no README row for the default feature `{}`", name));
        assert!(
            row.contains("| yes |"),
            "`{}` is on by default and the README's row does not say so: {}",
            name,
            row,
        );
    }

    for row in readme.lines().filter(|line| line.contains("| yes |")) {
        let name = row.split('`').nth(1).unwrap_or_else(|| {
            panic!("a `| yes |` row with no feature name in backticks: {}", row)
        });
        assert!(
            defaults.contains(&name),
            "the README says `{}` is on by default and the manifest's default list does not \
             contain it: {:?}",
            name,
            defaults,
        );
    }
}
