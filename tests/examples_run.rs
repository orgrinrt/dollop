//! The examples are built by `cargo test` and never run by it, so they are run here.
//!
//! For an allocator the risk is specific and invisible from a passing run: the numbers are
//! the whole content, and an example that leaked a block every round would print a
//! plausible falling count and look fine. These check the count comes back.

use std::process::Command;

/// Runs one example and returns what it printed.
fn run_example(name: &str, features: &[&str]) -> String {
    let mut command = Command::new(env!("CARGO"));
    command
        .args(&["run", "-q", "--example", name])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("CARGO_TARGET_DIR", concat!(env!("CARGO_MANIFEST_DIR"), "/target/examples"));
    if !features.is_empty() {
        command.args(&["--features", &features.join(",")]);
    }

    let output = command
        .output()
        .unwrap_or_else(|e| panic!("could not run example {}: {}", name, e));

    assert!(
        output.status.success(),
        "example {} exited {}\n--- stderr\n{}",
        name,
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );

    String::from_utf8(output.stdout).expect("example printed something that is not utf-8")
}

#[test]
fn one_region_takes_blocks_and_gives_them_all_back() {
    let out = run_example("one_region", &[]);

    // The free count at the start and at the end. Equal, or a block was lost, and a leak of
    // one block is exactly what a plausible-looking run hides.
    let free_at_start = out
        .lines()
        .find_map(|line| line.strip_prefix("a 4096 byte region, with "))
        .and_then(|rest| rest.split(' ').next())
        .expect("the opening line");

    let free_at_end = out
        .lines()
        .filter_map(|line| line.strip_prefix("returned "))
        .last()
        .and_then(|line| line.split(", ").nth(1))
        .and_then(|rest| rest.split(' ').next())
        .expect("the last return line");

    assert_eq!(free_at_start, free_at_end, "a block was not returned:\n{}", out);

    // And each allocation was aligned as asked, which is the property the example claims
    // and the one a wrong free-list would break silently.
    assert_eq!(
        out.matches("address aligned: true").count(),
        3,
        "not every block was aligned as requested:\n{}",
        out,
    );

    assert!(out.contains("one megabyte: None"), "an impossible request was served:\n{}", out);
}

#[test]
fn the_lending_example_returns_the_region_whole() {
    let out = run_example("lending_from_an_allocator", &["no_alloc"]);

    // The batches, with their sums, so a wrong fill fails here rather than only a missing
    // one.
    assert!(out.contains("batch 0: 4 readings, 2 sensors, sum 34"), "{}", out);
    assert!(out.contains("batch 1: 4 readings, 3 sensors, sum 223"), "{}", out);

    // The region whole again, which is the claim a leak would break. Both lines carry the
    // same number, and it is read rather than assumed.
    let free = out
        .lines()
        .find_map(|line| line.strip_suffix(" bytes free, the same as at the start"))
        .expect("the closing count");

    assert!(
        out.contains(&format!("refused, and {} bytes are still free", free)),
        "a refused lease changed the free count:\n{}",
        out,
    );
}
