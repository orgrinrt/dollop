//! Every feature selection this crate offers, built, and the declared minimum
//! checked.
//!
//! `no_std` is in the default set here, which makes the selection without it
//! the one that goes unbuilt: every mistake in a `std`-only path compiles for
//! nobody and is noticed by nobody. And `tlsf` gates the only allocator, so the
//! selection without it is a crate with a trait and no implementor, which still
//! has to compile.

use std::process::Command;

/// Builds the crate under one feature selection.
fn check(label: &str, args: &[&str]) {
    let output = Command::new(env!("CARGO"))
        .arg("check")
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env(
            "CARGO_TARGET_DIR",
            concat!(env!("CARGO_MANIFEST_DIR"), "/target/feature-matrix"),
        )
        .output()
        .unwrap_or_else(|e| panic!("could not run cargo for {}: {}", label, e));

    assert!(
        output.status.success(),
        "{} does not build\n--- cargo said\n{}",
        label,
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn the_default_selection_builds() {
    check("default", &[]);
}

#[test]
fn every_selection_builds() {
    for features in [
        // The trait alone, with no allocator behind it.
        "",
        "no_std",
        // Against `std`, which the default set never reaches, so a mistake in a std-only
        // path here compiles for nobody and is noticed by nobody.
        "tlsf",
        "no_alloc",
        "tlsf,no_alloc",
        "tlsf,no_std,no_alloc",
    ] {
        let label = if features.is_empty() { "no features" } else { features };
        if features.is_empty() {
            check(label, &["--no-default-features"]);
        } else {
            check(label, &["--no-default-features", "--features", features]);
        }
    }
}

#[test]
fn the_lending_suite_actually_runs_under_no_alloc() {
    // `tests/lending.rs` is `#![cfg(all(tlsf, no_alloc))]`, so under any other
    // selection it reports `running 0 tests`, which is what a suite that
    // stopped compiling reports too.
    let output = Command::new(env!("CARGO"))
        .args(["test", "--test", "lending", "--features", "no_alloc"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env(
            "CARGO_TARGET_DIR",
            concat!(env!("CARGO_MANIFEST_DIR"), "/target/feature-matrix"),
        )
        .output()
        .expect("cargo runs");

    let report = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "the lending suite passes:\n{}",
        report
    );

    let ran: usize = report
        .lines()
        .find_map(|line| line.strip_prefix("test result: ok. "))
        .and_then(|rest| rest.split(' ').next())
        .and_then(|count| count.parse().ok())
        .expect("the suite reported a result line");

    assert!(
        ran >= 9,
        "the lending suite ran {} cases, where it has at least 9. A suite that compiles \
         and executes nothing reports success just as loudly.",
        ran,
    );
}

#[test]
#[ignore = "catalogue: needs the 1.64.0 toolchain; run with --ignored"]
fn the_declared_minimum_toolchain_builds_the_default_selection() {
    // `rust-version = "1.64.0"` used to be enforced by a `rust-toolchain.toml`
    // pinning that version, which meant every build here was an MSRV build and
    // none was ever a current one. It also made `no_alloc` impossible to build
    // at all, because notko's manifest is edition 2024 and 1.64's cargo cannot
    // parse it.
    //
    // The pin is `stable` now and the minimum is checked here instead, over the
    // default selection, which is the set the declaration is about.
    const MSRV: &str = "1.64.0";

    let installed = Command::new("rustup")
        .args(["toolchain", "list"])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).contains(MSRV))
        .unwrap_or(false);

    assert!(
        installed,
        "the {} toolchain is not installed, so the `rust-version` claim cannot be checked \
         here. `rustup toolchain install {}` and run again.",
        MSRV, MSRV,
    );

    // Built as a crate of its own rather than in place. The lock file names notko,
    // which `no_alloc` pulls in and which is edition 2024, and 1.64's cargo
    // refuses to parse it whichever features are selected. That is a fact about
    // the lock rather than about whether the default selection's source
    // compiles, and the default selection has no dependencies at all, so a copy
    // of the manifest and the two modules it uses is the whole of it.
    let root = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/target/msrv-crate"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).expect("the msrv crate directory");

    std::fs::write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"msrv_check\"\nversion = \"0.0.0\"\nedition = \"2018\"\n\
             rust-version = \"{}\"\n\n[dependencies]\n\n[features]\n\
             default = [\"tlsf\", \"no_std\"]\ntlsf = []\nno_std = []\n\n[workspace]\n",
            MSRV,
        ),
    )
    .expect("the msrv manifest");

    for module in ["lib.rs", "strategy.rs", "tlsf.rs"] {
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join(module),
        )
        .unwrap_or_else(|e| panic!("reading src/{}: {}", module, e));
        std::fs::write(root.join("src").join(module), source)
            .unwrap_or_else(|e| panic!("writing src/{}: {}", module, e));
    }

    let output = Command::new("cargo")
        .args([format!("+{}", MSRV), "check".into()])
        .current_dir(&root)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .output()
        .expect("cargo runs");

    assert!(
        output.status.success(),
        "the default selection does not build under its own declared minimum, {}:\n{}",
        MSRV,
        String::from_utf8_lossy(&output.stderr),
    );
}
