//! `plantc` as people run it: real processes, real files.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use after_plants::spec::{AllometryPoint, Environment, PlantSpec};

fn plantc(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_plantc"))
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "plantc failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("plantc-{name}-{}", std::process::id()));
    if path.exists() {
        fs::remove_dir_all(&path).unwrap();
    }
    fs::create_dir_all(&path).unwrap();
    path
}

/// A spec file for a six-year-old Douglas-fir: one seed, in the open.
fn small_spec_file(dir: &Path) -> PathBuf {
    let mut spec = PlantSpec::builtin("pseudotsuga-menziesii").unwrap();
    spec.growth.years = 6.0;
    spec.growth.keyframes = vec![3.0, 6.0];
    spec.variants.environments = vec![Environment::Open];
    spec.variants.seeds = vec![7];
    spec.allometry = vec![AllometryPoint {
        age: 6.0,
        environment: Environment::Open,
        height: 4.0,
        dbh: None,
        tolerance: 0.9,
    }];
    let path = dir.join("young-fir.json");
    fs::write(&path, serde_json::to_string_pretty(&spec).unwrap()).unwrap();
    path
}

#[test]
fn list_and_check_name_the_built_in_species() {
    let listed = stdout(&plantc(&["list"]));
    assert!(listed.contains("pseudotsuga-menziesii"), "{listed}");
    assert!(listed.contains("Programs:"));
    let checked = stdout(&plantc(&["check", "pseudotsuga-menziesii"]));
    assert!(!checked.trim().is_empty());
    let help = stdout(&plantc(&["help"]));
    assert!(help.contains("plantc build"));
}

#[test]
fn grow_says_when_a_plant_has_died() {
    // Under a 30 m canopy the bigleaf maple sheds everything within five
    // years; the sizes would otherwise read as a row of zeros.
    let grown = stdout(&plantc(&[
        "grow",
        "acer-macrophyllum",
        "--env",
        "suppressed",
        "--years",
        "5",
    ]));
    assert!(
        grown.contains("age   5.0  nothing left: the plant has shed every part"),
        "{grown}"
    );
    assert!(!grown.contains("-0"), "{grown}");
}

#[test]
fn mistakes_fail_with_a_message_and_no_output() {
    for args in [
        &["grow", "no-such-species"][..],
        &["build", "pseudotsuga-menziesii", "--colour", "red"],
        &["frobnicate"],
        &["inspect", "/no/such/package.afterplant"],
    ] {
        let output = plantc(args);
        assert!(!output.status.success(), "{args:?} succeeded");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.starts_with("plantc: "), "{args:?}: {error}");
        assert!(output.stdout.is_empty(), "{args:?}");
    }
}

#[test]
fn build_writes_a_package_once_and_inspect_verifies_it() {
    let dir = scratch("build");
    let spec = small_spec_file(&dir);
    let out = dir.join("plants");
    let spec = spec.to_str().unwrap();
    let out_text = out.to_str().unwrap();
    let args = [
        "build",
        spec,
        "--out",
        out_text,
        "--quality",
        "draft",
        "--threads",
        "2",
    ];
    let built = stdout(&plantc(&args));
    assert!(built.contains("built pseudotsuga-menziesii"), "{built}");
    let packages: Vec<PathBuf> = fs::read_dir(&out)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(packages.len(), 1);
    let package = &packages[0];
    let name = package.file_name().unwrap().to_str().unwrap();
    assert!(
        name.starts_with("pseudotsuga-menziesii-") && name.ends_with(".afterplant"),
        "{name}"
    );
    // The printed key names the directory.
    let key = built
        .lines()
        .find_map(|line| line.trim().strip_prefix("key "))
        .unwrap();
    assert!(name.contains(&key[..16]));

    let again = stdout(&plantc(&args));
    assert!(again.contains("is already built"), "{again}");
    assert!(again.contains(key));

    let inspected = stdout(&plantc(&["inspect", package.to_str().unwrap()]));
    assert!(
        inspected.contains("every object matches its hash"),
        "{inspected}"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn render_writes_a_png() {
    let dir = scratch("render");
    let spec = small_spec_file(&dir);
    let image = dir.join("fir.png");
    stdout(&plantc(&[
        "render",
        spec.to_str().unwrap(),
        "--out",
        image.to_str().unwrap(),
        "--age",
        "6",
        "--size",
        "96",
        "--quality",
        "draft",
    ]));
    let bytes = fs::read(&image).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn atlas_draws_the_looks_and_compares_their_areas() {
    let dir = scratch("atlas");
    let image = dir.join("maple.png");
    let printed = stdout(&plantc(&[
        "atlas",
        "acer-macrophyllum",
        "--out",
        image.to_str().unwrap(),
    ]));
    assert!(printed.contains("leaf"), "{printed}");
    assert!(printed.contains("palmate"), "{printed}");
    assert!(
        printed.contains("drawn") && printed.contains("shaded"),
        "{printed}"
    );
    // The built-in species agree, so nothing is flagged.
    assert!(!printed.contains("differs"), "{printed}");
    let bytes = fs::read(&image).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");

    // A look for an organ the program does not declare is a mistake.
    let mut spec: serde_json::Value =
        serde_json::from_str(after_plants::spec::builtin_species("acer-macrophyllum").unwrap())
            .unwrap();
    spec["appearance"]["organs"]["petal"] =
        serde_json::json!({ "shape": { "template": "flower" } });
    let path = dir.join("bad.json");
    fs::write(&path, spec.to_string()).unwrap();
    let failed = plantc(&[
        "atlas",
        path.to_str().unwrap(),
        "--out",
        image.to_str().unwrap(),
    ]);
    assert!(!failed.status.success());
    let message = String::from_utf8_lossy(&failed.stderr);
    assert!(message.contains("petal"), "{message}");
    fs::remove_dir_all(&dir).unwrap();
}
