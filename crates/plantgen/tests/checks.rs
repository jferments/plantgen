//! The size checks (spec rework M8B) over the built-in species: every
//! species that states a size a check measures grows within that size's
//! extremes, seed after seed.

use plantgen::checks::{self, Verdict};
use plantgen::library::Library;
use plantgen::spec::PlantSpec;

/// Sizes known to fall past their extremes, each with why, until its
/// species is retuned. A pair here that comes to pass fails the test too,
/// so the list only shrinks.
const KNOWN: &[(&str, &str, &str)] = &[(
    "anaphalis-margaritacea",
    "involucre_height_m",
    "its heads' involucre is drawn 3.5 to 4.1 mm tall against FNA's 5 to 7 mm",
)];

/// Every species that states sizes, three plants each, has none past its
/// ranges' extremes but the [`KNOWN`] ones.
#[test]
fn stated_sizes_hold_within_their_extremes() {
    let library = Library::builtin();
    let mut checked = 0;
    let mut errors = Vec::new();
    let mut known = Vec::new();
    for entry in library.species() {
        let traits = library.traits_of(&entry.id).unwrap();
        if !checks::states_sizes(&traits) {
            continue;
        }
        let spec = PlantSpec::builtin(&entry.id).unwrap();
        let seeds = checks::seeds_of(&spec, 3);
        let report = checks::check(&spec, library, &traits, &seeds).unwrap();
        checked += 1;
        for finding in &report.findings {
            let listed = KNOWN
                .iter()
                .any(|(id, character, _)| *id == spec.id && *character == finding.character);
            if finding.verdict == Verdict::Error {
                if listed {
                    known.push((spec.id.clone(), finding.character));
                } else {
                    errors.push(format!(
                        "{} {}: {}",
                        spec.id, finding.character, finding.reason
                    ));
                }
            }
        }
    }
    assert!(checked >= 15, "only {checked} species state sizes");
    assert!(
        errors.is_empty(),
        "sizes past their extremes:\n{}",
        errors.join("\n")
    );
    for (id, character, why) in KNOWN {
        assert!(
            known.iter().any(|(k, c)| k == id && c == character),
            "{id} {character} now holds ({why}): take it off the known list"
        );
    }
}
