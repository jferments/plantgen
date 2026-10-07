//! The package gate: every built-in plant package, and every ground and
//! litter look, keeps its bytes while the generator is made ready to move
//! into its own repository (`docs/developer/PLANTS.md`, "The package
//! gate").
//!
//! `package-bytes.txt` lists each built-in package's directory name, the
//! day it is built for and the SHA-256 of its `manifest.json`; the
//! manifest names every object by its SHA-256, so equal manifests mean
//! equal packages. `look-bytes.txt` lists the SHA-256 of every ground and
//! litter look's texels with its tile, mean and relief. Both hold what P4's
//! merge (`1f37cf2`) builds. A change meant to change them writes them again
//! in the same pull request and says why:
//!
//! ```text
//! AFTER_PLANT_BYTES=write cargo test -p after-plants --test package_bytes --locked -- --include-ignored
//! ```

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use after_plants::ground::{GROUND_LOOK_NAMES, GroundLook, ground_look};
use after_plants::litter::{LITTER_LOOKS, litter_look};
use after_plants::package::{self, Inputs};
use after_plants::quality;
use after_plants::spec::{PlantSpec, all_species};
use sha2::{Digest, Sha256};

/// A winter day, when most seasoned organs are gone: the gate builds each
/// species with seasons on it too, so the day's path stays covered.
const WINTER_DAY: f64 = 20.0;

/// The seed the looks are drawn for.
const LOOK_SEED: u64 = 0;

/// Building every built-in package takes minutes, so this runs only when
/// asked for: the `plant-packages` phase (`tools/testing/run_suite.py`).
#[test]
#[ignore = "builds every built-in package; run it with the plant-packages phase"]
fn every_built_in_package_keeps_its_bytes() {
    let mut lines = Vec::new();
    for (id, _) in all_species() {
        let spec = PlantSpec::builtin(id).unwrap();
        let mut days = vec![package::DEFAULT_DAY];
        if spec.appearance.has_seasons() {
            days.push(WINTER_DAY);
        }
        for day in days {
            let inputs = Inputs::on_day(&spec, &quality::STANDARD, day).unwrap();
            let built = package::build(&inputs, package::default_threads(), &mut |_| {})
                .unwrap_or_else(|error| panic!("{id} on day {day}: {error}"));
            let manifest = built.manifest_bytes().unwrap();
            lines.push(format!(
                "{} {day} {}",
                built.manifest.directory_name(),
                hex(&Sha256::digest(&manifest))
            ));
        }
    }
    check_or_write("package-bytes.txt", &lines);
}

/// Each look is drawn on its own thread, as `WorldLab` draws them.
#[test]
fn every_ground_and_litter_look_keeps_its_pixels() {
    let ground = (0..GROUND_LOOK_NAMES.len()).map(|index| ("ground", index));
    let litter = (0..LITTER_LOOKS).map(|index| ("litter", index));
    let jobs: Vec<(&str, usize)> = ground.chain(litter).collect();
    let lines: Vec<String> = std::thread::scope(|scope| {
        let drawn: Vec<_> = jobs
            .iter()
            .map(|&(set, index)| {
                scope.spawn(move || {
                    let look = if set == "ground" {
                        ground_look(index, LOOK_SEED)
                    } else {
                        litter_look(index, LOOK_SEED)
                    };
                    look_line(set, LOOK_SEED, &look.unwrap())
                })
            })
            .collect();
        drawn
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect()
    });
    check_or_write("look-bytes.txt", &lines);
}

fn look_line(set: &str, seed: u64, look: &GroundLook) -> String {
    format!(
        "{set} {seed} {} tile {} mean {:?} relief {} {}",
        look.name,
        look.tile_m,
        look.mean,
        look.relief_m,
        hex(&Sha256::digest(&look.rgba))
    )
}

/// Compare `lines` with the committed list `name`, or write the list when
/// `AFTER_PLANT_BYTES=write`.
fn check_or_write(name: &str, lines: &[String]) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(name);
    let text = lines.iter().fold(String::new(), |mut text, line| {
        text.push_str(line);
        text.push('\n');
        text
    });
    if std::env::var("AFTER_PLANT_BYTES").as_deref() == Ok("write") {
        fs::write(&path, &text).unwrap();
        return;
    }
    let expected = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error}; it is made with AFTER_PLANT_BYTES=write. What this build gives:\n{text}",
            path.display()
        )
    });
    if expected == text {
        return;
    }
    let mut report = String::new();
    let old: Vec<&str> = expected.lines().collect();
    let new: Vec<&str> = text.lines().collect();
    for line in &old {
        if !new.contains(line) {
            let _ = writeln!(report, "  was: {line}");
        }
    }
    for line in &new {
        if !old.contains(line) {
            let _ = writeln!(report, "  now: {line}");
        }
    }
    panic!(
        "{name} no longer matches what this build gives. A pull request that keeps every \
         package's bytes must not change it; one meant to change them writes it again with \
         AFTER_PLANT_BYTES=write and says why.\n{report}"
    );
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}
