//! Bark looks in the library's specs (`appearance.bark_look`, the bark
//! language's drawing record keyed by stage): every species draws one, or
//! none, without error, and the reference species draw theirs.

use plantgen::library::Library;

/// The bark language's reference species and their stages, youngest first.
const REFERENCES: [(&str, &[&str]); 11] = [
    ("acer-macrophyllum", &["young", "mature"]),
    ("alnus-rubra", &["grey", "plated base"]),
    ("arbutus-menziesii", &["red", "retained grey"]),
    (
        "betula-papyrifera",
        &["twig", "young", "white", "black base"],
    ),
    ("pinus-ponderosa", &["twig", "blackjack", "plates"]),
    ("populus-tremuloides", &["twig", "white", "furrowed"]),
    ("prunus-emarginata", &["twig", "trunk"]),
    ("pseudotsuga-menziesii", &["young", "mature"]),
    ("quercus-garryana", &["young", "mature"]),
    ("rosa-nutkana", &["cane", "old wood"]),
    ("thuja-plicata", &["young", "old"]),
];

#[test]
fn every_species_draws_its_bark_look() {
    let library = Library::builtin();
    let mut drawn = 0;
    for entry in library.species() {
        let spec = library.spec(&entry.id).unwrap();
        let look = spec.appearance.bark_look(&spec.id).unwrap();
        if let Some(look) = look {
            assert_eq!(look.id, spec.id);
            drawn += 1;
        }
    }
    // The species with a bark pattern or a bark look, less the palms.
    assert!(drawn >= 60, "{drawn}");
}

#[test]
fn reference_species_draw_their_stated_stages() {
    let library = Library::builtin();
    for (id, stages) in REFERENCES {
        let spec = library.spec(id).unwrap();
        assert!(spec.appearance.bark_look.is_some(), "{id}");
        let look = spec.appearance.bark_look(id).unwrap().unwrap();
        let names: Vec<&str> = look
            .stages
            .iter()
            .map(|stage| stage.name.as_str())
            .collect();
        assert_eq!(names, stages, "{id}");
        // Each stage's surface is the species' own colour, stated.
        for stage in &look.stages {
            assert!(stage.colour.is_some(), "{id} {}", stage.name);
        }
    }
}

#[test]
fn a_genus_s_stages_reach_its_other_species() {
    let library = Library::builtin();
    // Pines state the genus's twig and plates, ponderosa adds blackjack.
    let pine = library.spec("pinus-monticola").unwrap();
    let look = pine
        .appearance
        .bark_look("pinus-monticola")
        .unwrap()
        .unwrap();
    let names: Vec<&str> = look
        .stages
        .iter()
        .map(|stage| stage.name.as_str())
        .collect();
    assert_eq!(names, ["twig", "plates"]);
    // Its surface is its own bark colour.
    let own = pine
        .appearance
        .bark
        .map(|c| plantgen::texture::linear_to_srgb(f64::from(c)));
    assert_eq!(look.stages[1].colour, Some(own));
}
