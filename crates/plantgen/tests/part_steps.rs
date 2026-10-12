//! Levels of detail by structure (spec rework SEMLOD) over the built-in
//! species: every coarser step of every part mesh looks like the part
//! mesh from all round above, with at most half the triangles of the step
//! before it.

use std::sync::Mutex;

use plantgen::library::Library;
use plantgen::mesh::Mesh;
use plantgen::parts::{self, STEP_COLOUR, STEP_COVER};
use plantgen::spec::PlantSpec;

fn triangles(variants: &[Mesh]) -> usize {
    variants.iter().map(Mesh::triangle_count).max().unwrap_or(0)
}

/// Every built-in species' part-mesh steps cover within [`STEP_COVER`] of
/// what the part mesh covers, in near its colours, from every direction;
/// each halves the triangles and its detail never shrinks; and heads,
/// clusters and flowers across the library have steps.
#[test]
fn part_steps_keep_cover_and_colour() {
    let library = Library::builtin();
    let ids: Vec<String> = library.species().iter().map(|e| e.id.clone()).collect();
    let next = Mutex::new(ids.iter());
    let failures = Mutex::new(Vec::new());
    let stepped = Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let Some(id) = next.lock().unwrap().next() else {
                        break;
                    };
                    let spec = PlantSpec::builtin(id).unwrap();
                    let years = spec.growth.years;
                    let env = plantgen::drawing::typical_environment(&spec, library);
                    let growth = plantgen::drawing::grow_variant(
                        &spec,
                        library,
                        None,
                        env,
                        1,
                        vec![years],
                        years,
                    )
                    .unwrap();
                    let (looks, _) = plantgen::drawing::looks_of(&spec, &growth, 196.0);
                    let meshes = parts::part_meshes(&looks);
                    for (part, steps) in meshes.iter().zip(parts::part_steps(&looks, &meshes)) {
                        let organ = &looks[part.template].organ;
                        let (mut last, mut detail) = (part.triangles(), steps.detail);
                        for step in &steps.steps {
                            let (cover, colour) =
                                parts::step_difference(&part.variants, &step.variants);
                            let mut wrong = Vec::new();
                            if cover > STEP_COVER || colour > STEP_COLOUR {
                                wrong.push(format!("cover {cover:.3}, colour {colour:.3}"));
                            }
                            if triangles(&step.variants) * 2 > last {
                                wrong.push(format!(
                                    "{} triangles after {last}",
                                    triangles(&step.variants)
                                ));
                            }
                            if step.detail < detail {
                                wrong.push(format!("detail {} after {detail}", step.detail));
                            }
                            if !wrong.is_empty() {
                                failures.lock().unwrap().push(format!(
                                    "{id} {organ} step {}: {}",
                                    step.coarse,
                                    wrong.join("; ")
                                ));
                            }
                            last = triangles(&step.variants);
                            detail = step.detail;
                        }
                        if !steps.steps.is_empty() {
                            stepped.lock().unwrap().push(format!("{id} {organ}"));
                        }
                    }
                }
            });
        }
    });
    let mut failures = failures.into_inner().unwrap();
    failures.sort();
    assert!(failures.is_empty(), "steps wrong:\n{}", failures.join("\n"));
    let stepped = stepped.into_inner().unwrap();
    assert!(
        stepped.len() >= 48,
        "only {} part meshes have steps: {stepped:?}",
        stepped.len()
    );
}
