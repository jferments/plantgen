//! `plantc` as people run it: real processes, real files.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use plantgen::spec::{AllometryPoint, Environment, PlantSpec};

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
    assert!(listed.contains("Cactaceae:"), "{listed}");
    assert!(listed.contains("carnegiea-gigantea"), "{listed}");
    assert!(listed.contains("Programs:"));
    let checked = stdout(&plantc(&["check", "pseudotsuga-menziesii"]));
    assert!(!checked.trim().is_empty());
    let help = stdout(&plantc(&["help"]));
    assert!(help.contains("plantc build"));
}

#[test]
fn a_library_folder_replaces_built_in_species() {
    let dir = scratch("library");
    let folder = dir.join("library/pinaceae/pseudotsuga/pseudotsuga-menziesii");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("spec.json"),
        plantgen::library::source("pseudotsuga-menziesii")
            .replace("\"Douglas-fir\"", "\"Douglas-fir from a folder\""),
    )
    .unwrap();
    let library = dir.to_str().unwrap();
    let listed = stdout(&plantc(&["list", "--library", library]));
    assert!(
        listed.contains("Douglas-fir from a folder (Pseudotsuga menziesii), program `conifer`, from the library folder"),
        "{listed}"
    );
    assert!(listed.contains("carnegiea-gigantea"), "{listed}");
    let checked = stdout(&plantc(&[
        "--library",
        library,
        "check",
        "pseudotsuga-menziesii",
    ]));
    assert!(
        checked.starts_with("pseudotsuga-menziesii: valid"),
        "{checked}"
    );
    // A family file above it: the spec says where each value came from.
    fs::write(
        dir.join("library/pinaceae/family.json"),
        r#"{"schema": 1, "rank": "family", "name": "Pinaceae",
            "generator": {"params": {"nod_years": 2.0}}}"#,
    )
    .unwrap();
    let checked = stdout(&plantc(&[
        "--library",
        library,
        "check",
        "pseudotsuga-menziesii",
    ]));
    assert!(
        checked.ends_with("  stands on pinaceae/family.json\n"),
        "{checked}"
    );
    let spec = stdout(&plantc(&[
        "--library",
        library,
        "spec",
        "pseudotsuga-menziesii",
    ]));
    let own = "pinaceae/pseudotsuga/pseudotsuga-menziesii/spec.json";
    assert!(
        spec.starts_with(&format!(
            "pseudotsuga-menziesii: {own} on pinaceae/family.json\n"
        )),
        "{spec}"
    );
    assert!(
        spec.contains("  generator.params.nod_years = 2.0  (pinaceae/family.json)\n"),
        "{spec}"
    );
    assert!(
        spec.contains(&format!("  generator.program = \"conifer\"  ({own})\n")),
        "{spec}"
    );
    assert!(
        spec.contains(&format!("evidence:\n  allometry  ({own})\n")),
        "{spec}"
    );
    let missing = plantc(&["list", "--library", dir.join("nowhere").to_str().unwrap()]);
    assert!(!missing.status.success());
    assert!(
        String::from_utf8_lossy(&missing.stderr)
            .contains("holds neither a library/ nor a programs/ folder")
    );
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
fn lineup_and_close_ups_write_pngs() {
    let dir = scratch("lineup");
    let image = dir.join("cacti.png");
    let printed = stdout(&plantc(&[
        "lineup",
        "mammillaria-grahamii",
        "opuntia-basilaris",
        "--seeds",
        "2",
        "--age",
        "5",
        "--size",
        "64",
        "--out",
        image.to_str().unwrap(),
    ]));
    // A row of two seeds for each species.
    assert_eq!(
        printed.matches("mammillaria-grahamii").count(),
        3,
        "{printed}"
    );
    assert!(printed.contains("seed 2"), "{printed}");
    let bytes = fs::read(&image).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");

    let close = dir.join("close.png");
    let printed = stdout(&plantc(&[
        "render",
        "mammillaria-grahamii",
        "--age",
        "10",
        "--focus",
        "0,0.05,0",
        "--span",
        "0.1",
        "--size",
        "64",
        "--out",
        close.to_str().unwrap(),
    ]));
    assert!(printed.contains("areoles of solid spines"), "{printed}");
    let failed = plantc(&[
        "render",
        "mammillaria-grahamii",
        "--focus",
        "0,0.05",
        "--out",
        close.to_str().unwrap(),
    ]);
    assert!(!failed.status.success());
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
        serde_json::from_str(plantgen::spec::builtin_species("acer-macrophyllum").unwrap())
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
