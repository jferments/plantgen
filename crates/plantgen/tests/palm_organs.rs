//! A palm's teeth and spines are parts of its frond: the organ check a
//! package runs (`organs::disagreements`, spec rework M7) passes for every
//! built-in palm whose fronds bear them. Project After's package gate
//! found Washingtonia's and Serenoa's petiole teeth and Phoenix's
//! acanthophylls refused as spines drawn within a leaf (2026-10-26).

use plantgen::grow::{GrowthSettings, grow};
use plantgen::lsys::Limits;
use plantgen::organs;
use plantgen::spec::PlantSpec;

/// Years grown: past the ages the gate refused (5 and 10 years).
const YEARS: f64 = 12.0;

#[test]
fn palm_teeth_and_spines_are_pieces_of_their_fronds() {
    let mut failures = Vec::new();
    for id in [
        "washingtonia-filifera",
        "phoenix-dactylifera",
        "serenoa-repens",
    ] {
        let spec = PlantSpec::builtin(id).unwrap();
        let (program, params) = spec.program().unwrap();
        let variant = &spec.variant_list()[0];
        let years = spec.growth.years.min(YEARS);
        let keyframes: Vec<f64> = (1..=years.floor() as u32).map(f64::from).collect();
        let growth = grow(
            &program,
            &params,
            &GrowthSettings {
                seed: variant.seed,
                dt: spec.growth.step,
                years,
                keyframes,
                conditions: variant.conditions(),
                limits: Limits::default(),
                host: spec.host_geometry().unwrap(),
            },
        )
        .unwrap();
        let terms = organs::of_program(&program, &spec.appearance);
        let organ_types: Vec<(&str, &str)> =
            program.organs().map(|(name, _)| name).zip(terms).collect();
        let structure_types: Vec<(&str, &str)> = growth
            .structure_types
            .iter()
            .map(|kind| (kind.name.as_str(), kind.term.as_str()))
            .collect();
        for graph in &growth.keyframes {
            for problem in organs::disagreements(graph, &organ_types, &structure_types) {
                failures.push(format!("{id} at {} years: {problem}", graph.age));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
