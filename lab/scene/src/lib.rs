//! PlantLab's engine-free layer: a plant grown by PlantGen, turned into
//! data ready to draw, with the camera and light that frame it.
//!
//! Nothing here knows Bevy or a GPU, so it is tested on the CPU and a
//! different engine could draw the same [`Scene`]. Nothing here computes
//! anything about the plant either: the plant comes from
//! [`plantgen::drawing::draw`], the steps `plantc render` takes, and its
//! card colours from [`Templates::albedo`]. A renderer of a scene only
//! rasterises, lights and blends:
//!
//! - **wood** and **solids** (spines, the scale figure) are opaque, in
//!   their vertex colours;
//! - **cards** are cut out by their template, layer `colour.a - 1` of
//!   [`Scene::templates`] (the layer plus one, so an alpha of at least 1
//!   never trips a renderer's alpha mask): discard where the texel's
//!   coverage (alpha) is below one half; else the albedo is
//!   `mix(colour.rgb, accent * darkening, texel.g) * 2 * texel.r`, the
//!   card's colour moved toward its template's accent colour
//!   ([`Scene::template_accents`]) darkened for this card
//!   ([`SceneMesh::darkening`]), times the texel's brightness. That is
//!   [`Templates::albedo`] exactly (a test holds it so). Cards are
//!   two-sided.

use std::fmt::Write as _;

use plantgen::drawing::{self, Drawing, Request};
pub use plantgen::grow::Progress;
use plantgen::grow::Watch;
use plantgen::library::Library;
use plantgen::math::Vec3;
use plantgen::mesh::{Mesh, PlantMesh};
use plantgen::quality::{self, Quality};
use plantgen::spec::{Environment, PlantSpec};
use plantgen::spines;
use plantgen::templates::{TEMPLATE_SIZE, Templates, Texel};

/// The review background, linear grey: about middle grey once encoded.
pub const BACKGROUND: [f32; 3] = [0.2, 0.2, 0.2];
/// The ground patch under the plant, linear RGB (`plantc`'s ground).
pub const GROUND: [f32; 3] = [0.16, 0.19, 0.11];
/// The scale figure and the dark bands of the rod, linear RGB.
pub const FIGURE: [f32; 3] = [0.55, 0.22, 0.12];
/// The light bands of the rod.
pub const ROD_LIGHT: [f32; 3] = [0.7, 0.7, 0.66];
/// Plants framed lower than this, in metres, get the striped rod beside
/// them instead of the 1.8 m figure, which would dwarf them.
pub const SMALL_PLANT: f64 = 1.5;
/// Vertical field of view of the perspective views, degrees.
pub const FOV_Y_DEG: f64 = 30.0;

/// One mesh of a scene, in metres, y up.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SceneMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Linear RGB and 1.
    pub colors: Vec<[f32; 4]>,
    /// Cards only: how much the template's accent colour is darkened for
    /// this card (`Templates::albedo`). Empty on other meshes.
    pub darkening: Vec<f32>,
    /// Wood only: the stem's radius at each vertex, metres, which the bark
    /// pattern is drawn by ([`Scene::bark`]); 0 off bark. Empty on other
    /// meshes and on wood without bark.
    pub bark_radius: Vec<f32>,
    pub indices: Vec<u32>,
}

impl SceneMesh {
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    fn from_mesh(mesh: &Mesh) -> Self {
        Self {
            positions: mesh.positions.clone(),
            normals: mesh.normals.clone(),
            uvs: mesh.uvs.clone(),
            colors: mesh
                .colors
                .iter()
                .map(|c| [c[0], c[1], c[2], 1.0])
                .collect(),
            darkening: Vec::new(),
            // PlantGen marks bark with the negative radius in alpha.
            bark_radius: if mesh.colors.iter().any(|c| c[3] < 0.0) {
                mesh.colors.iter().map(|c| (-c[3]).max(0.0)).collect()
            } else {
                Vec::new()
            },
            indices: mesh.indices.clone(),
        }
    }

    fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut points = self.positions.iter().map(|p| vec3(*p));
        let first = points.next()?;
        Some(points.fold((first, first), |(low, high), p| {
            (
                Vec3::new(low.x.min(p.x), low.y.min(p.y), low.z.min(p.z)),
                Vec3::new(high.x.max(p.x), high.y.max(p.y), high.z.max(p.z)),
            )
        }))
    }
}

/// The organ templates as layers of one texture array: per texel,
/// brightness / 2 in red, the accent weight in green, 0 in blue and
/// coverage in alpha (the package atlas's encoding), row 0 at the cards'
/// base. Sampled nearest, as [`Templates::sample`] does.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateLayers {
    pub size: usize,
    /// `layers * size * size * 4` bytes, layer by layer.
    pub rgba: Vec<u8>,
    pub layers: usize,
}

impl TemplateLayers {
    /// One layer per template, at least one (fully covered and plain) so
    /// the texture always exists.
    #[must_use]
    pub fn of(templates: &Templates) -> Self {
        let size = templates.size;
        let layers = templates.templates.len().max(1);
        let mut rgba = Vec::with_capacity(layers * size * size * 4);
        if templates.templates.is_empty() {
            rgba.extend([128, 0, 0, 255].repeat(size * size));
        }
        for template in &templates.templates {
            for at in 0..size * size {
                rgba.extend([
                    plantgen::raster::to_u8(template.brightness[at] * 0.5),
                    plantgen::raster::to_u8(template.accent[at]),
                    0,
                    plantgen::raster::to_u8(template.coverage[at]),
                ]);
            }
        }
        Self { size, rgba, layers }
    }
}

/// A card's accent colour, darkened as [`Templates::albedo`] darkens it
/// for a card of `colour`: the albedo of a fully accented texel of
/// brightness 1.
#[must_use]
pub fn card_accent(templates: &Templates, template: usize, colour: [f32; 3]) -> [f32; 3] {
    let full = Texel {
        coverage: 1.0,
        brightness: 1.0,
        accent: 1.0,
    };
    // Colours are small; f32 keeps them.
    #[allow(clippy::cast_possible_truncation)]
    templates
        .albedo(template, colour.map(f64::from), full)
        .map(|c| c as f32)
}

/// How much `template`'s accent colour is darkened on a card of
/// `colour`: [`card_accent`] over the accent colour, read on its
/// strongest channel (1 when the template has no accent colour).
#[must_use]
pub fn darkening(templates: &Templates, template: usize, colour: [f32; 3]) -> f32 {
    let Some(look) = templates.templates.get(template) else {
        return 1.0;
    };
    let accent = card_accent(templates, template, colour);
    let (k, strongest) =
        look.accent_colour
            .iter()
            .copied()
            .enumerate()
            .fold(
                (0, 0.0_f32),
                |best, (k, c)| if c > best.1 { (k, c) } else { best },
            );
    if strongest > 1e-6 {
        accent[k] / strongest
    } else {
        1.0
    }
}

/// Each template's accent colour, linear RGB, one per layer.
#[must_use]
pub fn template_accents(templates: &Templates) -> Vec<[f32; 3]> {
    if templates.templates.is_empty() {
        return vec![[0.0; 3]];
    }
    templates
        .templates
        .iter()
        .map(|template| template.accent_colour)
        .collect()
}

/// Most templates a scene may hold: the size of a renderer's accent
/// table.
pub const MAX_TEMPLATES: usize = 64;

/// Where the camera stands and what it looks at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Framing {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub up: [f32; 3],
    pub projection: Projection,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Projection {
    /// Vertical field of view, degrees.
    Perspective { fov_y_deg: f32 },
    /// Half the view's height, metres.
    Orthographic { half_height: f32 },
}

/// The fixed review light: one sun that casts shadows, and the sky.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    /// Unit vector toward the sun.
    pub sun: [f32; 3],
    /// Linear RGB.
    pub sun_color: [f32; 3],
    /// Sun illuminance, lux.
    pub sun_lux: f32,
    /// Even sky light, linear RGB, and its brightness in cd/m².
    pub sky_color: [f32; 3],
    pub sky_brightness: f32,
}

/// The review sun: from the south-east, 35° up.
#[must_use]
pub fn review_light() -> Light {
    let elevation = 35.0_f64.to_radians();
    // South-east: +x east and +z south, as `plantc`'s previews light.
    let (east, south) = (0.5_f64.sqrt(), 0.5_f64.sqrt());
    let sun = Vec3::new(
        east * elevation.cos(),
        elevation.sin(),
        south * elevation.cos(),
    );
    Light {
        sun: sun.to_f32(),
        sun_color: [1.0, 0.95, 0.85],
        sun_lux: 10_000.0,
        sky_color: [0.62, 0.7, 0.85],
        sky_brightness: 2_500.0,
    }
}

/// How a scene is framed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Level with the plant, from the south.
    Side,
    /// From the south-east, a little above.
    ThreeQuarter,
    /// Straight down.
    Top,
}

impl View {
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "side" => Self::Side,
            "three-quarter" => Self::ThreeQuarter,
            "top" => Self::Top,
            _ => return None,
        })
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Side => "side",
            Self::ThreeQuarter => "three-quarter",
            Self::Top => "top",
        }
    }
}

/// How a picture is lit and finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// The fixed review look: hard-edged shadows, even sky light, no tone
    /// mapping, so species compare fairly.
    Review,
    /// A photo: soft shadows from the sun's real size, sky light from an
    /// environment, ambient occlusion and a filmic tone map.
    Photo,
}

impl Look {
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "review" => Self::Review,
            "photo" => Self::Photo,
            _ => return None,
        })
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Review => "review",
            Self::Photo => "photo",
        }
    }

    /// Times the picture is drawn larger and averaged down, per side.
    #[must_use]
    pub fn supersample(self) -> u32 {
        match self {
            Self::Review => 1,
            Self::Photo => 3,
        }
    }
}

/// How much brighter the photo looks are exposed than the review look,
/// in stops. The review look shows linear light as it is; a filmic tone
/// map (ACES) draws middle grey about a stop and a half darker, so without
/// this the photos read dark (measured on a path-traced ground, 2026-10-08).
pub const PHOTO_EXPOSURE_BOOST_EV: f32 = 1.5;

/// The sun's angular diameter, radians: 0.53°.
pub const SUN_ANGULAR_DIAMETER: f32 = 0.00925;

/// What to draw and how to frame it.
#[derive(Debug, Clone)]
pub struct Shot {
    /// A library id or a spec's JSON text.
    pub species: String,
    /// `None`: the species' typical site.
    pub environment: Option<Environment>,
    /// `None`: the spec's first seed.
    pub seed: Option<u64>,
    /// `None`: the oldest keyframe.
    pub age: Option<f64>,
    pub day: f64,
    pub level: usize,
    pub quality: Quality,
    pub view: View,
    /// Width over height of the picture.
    pub aspect: f64,
    /// Draw the scale figure or rod beside the plant.
    pub scale: bool,
    pub look: Look,
    /// Frame this height, metres, instead of the plant's own, so plants
    /// of different ages share a scale.
    pub frame_height: Option<f64>,
    /// A close-up instead: this point of the plant, metres, with this many
    /// metres of it from the bottom of the picture to the top, and no
    /// scale.
    pub focus: Option<([f64; 3], f64)>,
    /// Also cut every card's outline into triangles ([`cut_cards`]), on a
    /// grid this many cells along each side: for renderers that cannot cut
    /// cards out by their texture, such as ray tracing. 0 for none.
    pub cut_cards: usize,
}

impl Shot {
    /// A thumbnail's shot: the typical plant, three-quarter view, square,
    /// with its scale.
    #[must_use]
    pub fn thumbnail(species: &str) -> Self {
        Self {
            species: species.to_string(),
            environment: None,
            seed: None,
            age: None,
            day: plantgen::package::DEFAULT_DAY,
            level: 0,
            quality: quality::STANDARD,
            view: View::ThreeQuarter,
            aspect: 1.0,
            scale: true,
            look: Look::Review,
            frame_height: None,
            focus: None,
            cut_cards: 0,
        }
    }
}

/// Everything a renderer needs for one picture.
#[derive(Debug, Clone)]
pub struct Scene {
    pub wood: SceneMesh,
    pub cards: SceneMesh,
    /// Opaque solids in vertex colours: spines.
    pub solids: SceneMesh,
    /// The scale beside the plant (a figure or a banded rod), in vertex
    /// colours; empty without one.
    pub scale: SceneMesh,
    /// The cards cut into opaque triangles in their colours, when the shot
    /// asks for it ([`Shot::cut_cards`]); empty otherwise.
    pub cut_cards: SceneMesh,
    pub templates: TemplateLayers,
    /// Each layer's accent colour, linear RGB.
    pub template_accents: Vec<[f32; 3]>,
    /// The bark pattern drawn on the wood ([`SceneMesh::bark_radius`]);
    /// `None` for plain bark.
    pub bark: Option<plantgen::bark::BarkParams>,
    /// The ground: a patch of this radius, metres, centred under the
    /// plant, reaching past its crown and its scale.
    pub ground_radius: f32,
    pub framing: Framing,
    pub light: Light,
    pub look: Look,
    /// Room the sun's shadow must cover, low and high corners.
    pub shadow_bounds: ([f32; 3], [f32; 3]),
    /// What was drawn, for the picture's sidecar.
    pub facts: Facts,
}

/// What a scene shows, for the record beside its picture.
#[derive(Debug, Clone, PartialEq)]
pub struct Facts {
    pub species: String,
    pub generator_revision: u32,
    pub environment: String,
    pub seed: u64,
    pub age: f64,
    pub day: f64,
    pub level: usize,
    pub quality: String,
    pub view: &'static str,
    pub look: &'static str,
    pub height_m: f64,
    pub crown_width_m: f64,
    pub triangles: usize,
    pub cards: usize,
    pub on_host: bool,
    pub export: ExportSizes,
}

/// How big the plant on screen would be as a file, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExportSizes {
    /// Exactly: this level's mesh as a PlantGen package stores it
    /// (`APMESH1`, [`plantgen::package::encoded_mesh_size`]).
    pub package_mesh: usize,
    /// Exactly: the card templates as one PNG, layers stacked.
    pub textures_png: usize,
    /// About: the drawn triangles as glTF binary (positions, normals,
    /// texture coordinates and RGBA colours as floats, 32-bit indices),
    /// with the textures PNG.
    pub glb: usize,
    /// About: the drawn triangles as Wavefront OBJ text with six decimals,
    /// with the textures PNG.
    pub obj: usize,
}

impl ExportSizes {
    /// The sizes of the plant drawn as `meshes`, packaged as `plant`, with
    /// card templates `layers`.
    #[must_use]
    pub fn of(meshes: &[&SceneMesh], plant: &PlantMesh, layers: &TemplateLayers) -> Self {
        let textures_png =
            plantgen::raster::encode_png(layers.size, layers.size * layers.layers, &layers.rgba)
                .map_or(layers.rgba.len(), |png| png.len());
        let vertices: usize = meshes.iter().map(|mesh| mesh.positions.len()).sum();
        let indices: usize = meshes.iter().map(|mesh| mesh.indices.len()).sum();
        // The JSON chunk: a few accessors, buffer views and a material
        // per mesh.
        let json = 1_200 + 900 * meshes.len();
        let glb = 12 + 8 + json + 8 + vertices * (12 + 12 + 8 + 16) + indices * 4 + textures_png;
        // "v x y z", "vn x y z", "vt u v" at six decimals, and "f a/a/a b/b/b
        // c/c/c" with indices of the vertex count's digits.
        let digits = vertices.max(1).to_string().len();
        let face = 2 + 3 * (3 * digits + 3);
        let obj = vertices * (36 + 30 + 20) + indices / 3 * face + 200 + textures_png;
        Self {
            package_mesh: plantgen::package::encoded_mesh_size(plant),
            textures_png,
            glb,
            obj,
        }
    }
}

/// Load a spec: a library id, or the JSON text of a spec.
///
/// # Errors
///
/// When the id is not in the library or the text is not a valid spec.
pub fn load_spec(species: &str, library: &Library) -> Result<PlantSpec, String> {
    if species.trim_start().starts_with('{') {
        PlantSpec::from_json_in(species, library).map_err(|error| error.to_string())
    } else {
        library.spec(species).map_err(|error| error.to_string())
    }
}

/// Grow and frame `shot` from `library`.
///
/// # Errors
///
/// When the species does not load or grow.
pub fn build(shot: &Shot, library: &Library) -> Result<Scene, String> {
    let spec = load_spec(&shot.species, library)?;
    let request = request(shot, &spec, library);
    let drawing = drawing::draw(&request)?;
    Ok(from_drawing(&drawing, &request, shot))
}

/// Watches [`build_watched`] grow a plant.
pub trait Viewer {
    /// Whether to draw the plant as it stands after `step`.
    fn wants_frame(&mut self, step: u32) -> bool;

    /// Called after each growth step, with the plant drawn when
    /// [`Viewer::wants_frame`] asked for it (alone, without its host).
    /// Returning false stops the growth. May block, to pause it.
    fn step(&mut self, progress: Progress<'_>, frame: Option<Scene>) -> bool;
}

/// [`build`], telling `viewer` after each growth step.
///
/// # Errors
///
/// As [`build`], and "growth stopped" when the viewer stops it.
pub fn build_watched(
    shot: &Shot,
    library: &Library,
    viewer: &mut dyn Viewer,
) -> Result<Scene, String> {
    struct Frames<'a> {
        request: Request<'a>,
        shot: &'a Shot,
        viewer: &'a mut dyn Viewer,
    }
    impl Watch for Frames<'_> {
        fn wants_frame(&mut self, step: u32) -> bool {
            self.viewer.wants_frame(step)
        }
        fn step(&mut self, progress: Progress<'_>) -> bool {
            let frame = progress.frame.and_then(|growth| {
                let alone = Request {
                    alone: true,
                    age: progress.age,
                    ..self.request
                };
                drawing::dress(&alone, growth.clone())
                    .ok()
                    .map(|drawing| from_drawing(&drawing, &alone, self.shot))
            });
            self.viewer.step(progress, frame)
        }
    }
    let spec = load_spec(&shot.species, library)?;
    let request = request(shot, &spec, library);
    let mut frames = Frames {
        request,
        shot,
        viewer,
    };
    let drawing = drawing::draw_watched(&request, Some(&mut frames))?;
    Ok(from_drawing(&drawing, &request, shot))
}

fn request<'a>(shot: &'a Shot, spec: &'a PlantSpec, library: &'a Library) -> Request<'a> {
    let mut request = Request::typical(spec, library, &shot.quality);
    if let Some(environment) = shot.environment {
        request.environment = environment;
    }
    if let Some(seed) = shot.seed {
        request.seed = seed;
    }
    if let Some(age) = shot.age {
        request.age = age;
    }
    request.day = shot.day;
    request.level = shot.level;
    request
}

/// Frame a plant already drawn.
#[must_use]
pub fn from_drawing(drawing: &Drawing, request: &Request<'_>, shot: &Shot) -> Scene {
    let templates = &drawing.templates;
    let plant = &drawing.plant;
    // A level with tufts draws them as solid spines in place of their spine
    // cards, from the body looks, as `plantc` does near the camera.
    let solid_spines = !plant.tufts.is_empty() && !templates.bodies.is_empty();
    let card_mesh = if solid_spines {
        let first = templates.first_spine_template();
        PlantMesh {
            cards: plant
                .cards
                .iter()
                .filter(|card| usize::from(card.template) < first)
                .copied()
                .collect(),
            ..PlantMesh::default()
        }
        .card_mesh()
    } else {
        plant.card_mesh()
    };
    let spines = if solid_spines {
        let looks: Vec<_> = templates.bodies.iter().collect();
        spines::tuft_mesh(&plant.tufts, &looks)
    } else {
        Mesh::default()
    };
    let wood = SceneMesh::from_mesh(&plant.wood);
    let cards = cards(&card_mesh, templates);
    let cut = if shot.cut_cards > 0 {
        let first = if solid_spines {
            templates.first_spine_template()
        } else {
            usize::MAX
        };
        cut_cards(plant, templates, shot.cut_cards, first)
    } else {
        SceneMesh::default()
    };
    let solids = SceneMesh::from_mesh(&spines);
    let layers = TemplateLayers::of(templates);
    let export = ExportSizes::of(&[&wood, &cards, &solids], plant, &layers);

    let (reach, high) = reach_and_top(&[&wood, &cards, &solids]);
    let framed = shot.frame_height.unwrap_or(high.y).max(0.05);
    let (scale_mesh, scale_height, gap) = scale(framed, reach);
    let scale_mesh = if shot.scale && shot.focus.is_none() {
        scale_mesh
    } else {
        SceneMesh::default()
    };
    let framing = frame(shot, reach, framed, scale_height, gap);
    let (shadow_low, shadow_high) = {
        let pad = framed * 1.5 + reach;
        (
            Vec3::new(-pad - gap, 0.0, -pad),
            Vec3::new(pad + gap + 1.0, framed.max(scale_height), pad),
        )
    };
    let facts = Facts {
        species: request.spec.id.clone(),
        generator_revision: plantgen::GENERATOR_REVISION,
        environment: format!("{:?}", request.environment).to_lowercase(),
        seed: request.seed,
        age: request.age,
        day: request.day,
        level: request.level,
        quality: request.quality.name.to_string(),
        view: shot.view.name(),
        look: shot.look.name(),
        height_m: drawing.graph.height,
        crown_width_m: 2.0 * reach,
        triangles: wood.triangle_count() + cards.triangle_count() + solids.triangle_count(),
        cards: plant.cards.len(),
        on_host: drawing.on_host,
        export,
    };
    #[allow(clippy::cast_possible_truncation)]
    Scene {
        wood,
        cards,
        solids,
        scale: scale_mesh,
        cut_cards: cut,
        templates: layers,
        bark: templates.bark,
        template_accents: template_accents(templates),
        ground_radius: (reach * 1.4 + gap + 0.6).max(reach + 0.3) as f32,
        framing,
        light: review_light(),
        look: shot.look,
        shadow_bounds: (shadow_low.to_f32(), shadow_high.to_f32()),
        facts,
    }
}

/// How far `meshes` reach from the plant's axis (at least 10 cm), and
/// their highest corner.
fn reach_and_top(meshes: &[&SceneMesh]) -> (f64, Vec3) {
    let (low, high) = meshes
        .iter()
        .filter_map(|mesh| mesh.bounds())
        .reduce(|(l0, h0), (l1, h1)| {
            (
                Vec3::new(l0.x.min(l1.x), l0.y.min(l1.y), l0.z.min(l1.z)),
                Vec3::new(h0.x.max(h1.x), h0.y.max(h1.y), h0.z.max(h1.z)),
            )
        })
        .unwrap_or((Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)));
    let reach = low
        .x
        .abs()
        .max(high.x.abs())
        .max(low.z.abs())
        .max(high.z.abs())
        .max(0.1);
    (reach, high)
}

/// Every card of `plant` (whose template is below `first_excluded`) cut
/// into opaque triangles along its template's outline: the card's grid of
/// `grid` by `grid` cells keeps each cell whose centre the template covers
/// (coverage at least one half, as a renderer cuts it), each corner placed
/// on the card as it bends ([`plantgen::bend::Bend::point`]) and coloured
/// by [`Templates::albedo`] there. One-sided, facing the card's front.
#[must_use]
pub fn cut_cards(
    plant: &PlantMesh,
    templates: &Templates,
    grid: usize,
    first_excluded: usize,
) -> SceneMesh {
    let grid = grid.max(1);
    let corners = grid + 1;
    #[allow(clippy::cast_precision_loss)]
    let step = 1.0 / grid as f64;
    let vector = |v: [f32; 3]| Vec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]));
    let mut out = SceneMesh::default();
    // Each template's kept cells, computed once.
    let mut kept: Vec<Option<Vec<bool>>> = vec![None; templates.templates.len() + 1];
    let mut index = vec![u32::MAX; corners * corners];
    for card in &plant.cards {
        let template = usize::from(card.template);
        if template >= first_excluded {
            continue;
        }
        let slot = template.min(templates.templates.len());
        let cells = kept[slot].get_or_insert_with(|| {
            (0..grid * grid)
                .map(|cell| {
                    let (i, j) = (cell % grid, cell / grid);
                    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
                    let (u, v) = (
                        ((i as f64 + 0.5) * step) as f32,
                        ((j as f64 + 0.5) * step) as f32,
                    );
                    templates.sample(template, u, v).coverage >= 0.5
                })
                .collect()
        });
        let (base, heading, left) = (vector(card.base), vector(card.heading), vector(card.left));
        let half = f64::from(card.width) * 0.5;
        let length = f64::from(card.length);
        let colour = card.color.map(f64::from);
        index.fill(u32::MAX);
        for (cell, &keep) in cells.iter().enumerate() {
            if !keep {
                continue;
            }
            let (i, j) = (cell % grid, cell / grid);
            let mut quad = [0_u32; 4];
            for (slot, (ci, cj)) in [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)]
                .into_iter()
                .enumerate()
            {
                let at = cj * corners + ci;
                if index[at] == u32::MAX {
                    #[allow(clippy::cast_precision_loss)]
                    let (u, v) = (ci as f64 * step, cj as f64 * step);
                    let (position, normal) =
                        card.bend
                            .point(base, heading, left, half, length, 2.0 * u - 1.0, v);
                    #[allow(clippy::cast_possible_truncation)]
                    let texel = templates.sample(template, u as f32, v as f32);
                    let albedo = templates.albedo(template, colour, texel);
                    index[at] = u32::try_from(out.positions.len()).unwrap_or(u32::MAX);
                    out.positions.push(position.to_f32());
                    out.normals.push(normal.to_f32());
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        out.uvs.push([u as f32, v as f32]);
                        out.colors.push([
                            albedo[0] as f32,
                            albedo[1] as f32,
                            albedo[2] as f32,
                            1.0,
                        ]);
                    }
                }
                quad[slot] = index[at];
            }
            // Counter-clockwise seen from the card's front, as the cards are.
            out.indices
                .extend_from_slice(&[quad[0], quad[2], quad[1], quad[0], quad[3], quad[2]]);
        }
    }
    out
}

/// The card mesh with each vertex's template layer (plus one) in its
/// colour's alpha and its accent darkening.
fn cards(mesh: &Mesh, templates: &Templates) -> SceneMesh {
    let mut out = SceneMesh::from_mesh(mesh);
    let mut darkening = Vec::with_capacity(mesh.colors.len());
    for (out_colour, c) in out.colors.iter_mut().zip(&mesh.colors) {
        // The template index is a small whole number in alpha.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let template = c[3].round() as usize;
        // A card naming no template is fully covered and plain: layer 0,
        // whose texels are, when there are no templates at all.
        let layer = if template < templates.templates.len() {
            template
        } else {
            0
        };
        #[allow(clippy::cast_precision_loss)]
        {
            out_colour[3] = (layer + 1) as f32;
        }
        darkening.push(self::darkening(templates, template, [c[0], c[1], c[2]]));
    }
    out.darkening = darkening;
    out
}

fn vec3(p: [f32; 3]) -> Vec3 {
    Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
}

fn push_quad(mesh: &mut SceneMesh, corners: [Vec3; 4], normal: Vec3, color: [f32; 3]) {
    let base = u32::try_from(mesh.positions.len()).unwrap_or(0);
    for corner in corners {
        mesh.positions.push(corner.to_f32());
        mesh.normals.push(normal.to_f32());
        mesh.uvs.push([0.0, 0.0]);
        mesh.colors.push([color[0], color[1], color[2], 1.0]);
    }
    mesh.indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// A box `2 * wide` across and `2 * deep` deep, centred on `x`, from
/// height `low` to `high`: four sides and a top.
fn push_box(
    mesh: &mut SceneMesh,
    x: f64,
    (wide, deep): (f64, f64),
    (low, high): (f64, f64),
    color: [f32; 3],
) {
    let at = |dx: f64, y: f64, dz: f64| Vec3::new(x + dx, y, dz);
    let faces = [
        (
            [
                at(-wide, low, deep),
                at(wide, low, deep),
                at(wide, high, deep),
                at(-wide, high, deep),
            ],
            Vec3::Z,
        ),
        (
            [
                at(wide, low, -deep),
                at(-wide, low, -deep),
                at(-wide, high, -deep),
                at(wide, high, -deep),
            ],
            -Vec3::Z,
        ),
        (
            [
                at(wide, low, deep),
                at(wide, low, -deep),
                at(wide, high, -deep),
                at(wide, high, deep),
            ],
            Vec3::X,
        ),
        (
            [
                at(-wide, low, -deep),
                at(-wide, low, deep),
                at(-wide, high, deep),
                at(-wide, high, -deep),
            ],
            -Vec3::X,
        ),
        (
            [
                at(-wide, high, deep),
                at(wide, high, deep),
                at(wide, high, -deep),
                at(-wide, high, -deep),
            ],
            Vec3::Y,
        ),
    ];
    for (corners, normal) in faces {
        push_quad(mesh, corners, normal, color);
    }
}

/// The scale beside a plant framed `height` tall whose crown reaches `x`:
/// a 1.8 m figure, or for a small plant a rod 0.5 m or 1 m tall in 10 cm
/// bands. Returns the mesh, its height and the gap left between plant and
/// scale.
#[must_use]
pub fn scale(height: f64, x: f64) -> (SceneMesh, f64, f64) {
    let mut mesh = SceneMesh::default();
    if height >= SMALL_PLANT {
        push_box(&mut mesh, x + 0.8, (0.225, 0.14), (0.0, 1.8), FIGURE);
        (mesh, 1.8, 0.8)
    } else {
        let tall: f64 = if height < 0.5 { 0.5 } else { 1.0 };
        let gap = (0.25 * height).max(0.08);
        // Whole bands only; the count is at most 10.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let bands = (tall / 0.1).round().max(1.0) as u32;
        for band in 0..bands {
            let low = f64::from(band) * 0.1;
            let color = if band % 2 == 0 { FIGURE } else { ROD_LIGHT };
            push_box(&mut mesh, x + gap, (0.012, 0.012), (low, low + 0.1), color);
        }
        (mesh, tall, gap)
    }
}

/// The camera that frames a plant `framed` metres tall reaching `reach`
/// from its axis, with its scale `gap` beyond, as `plantc`'s previews do.
fn frame(shot: &Shot, reach: f64, framed: f64, scale_height: f64, gap: f64) -> Framing {
    if let Some((point, span)) = shot.focus {
        let target = Vec3::new(point[0], point[1], point[2]);
        let direction = if shot.view == View::Side {
            Vec3::new(0.0, 0.04, 1.0)
        } else {
            Vec3::new(0.7, 0.3, 0.7)
        }
        .normalize_or(Vec3::Z);
        let half_fov = (FOV_Y_DEG.to_radians() * 0.5).tan();
        #[allow(clippy::cast_possible_truncation)]
        return Framing {
            eye: (target + direction * (span * 0.5 / half_fov)).to_f32(),
            target: target.to_f32(),
            up: [0.0, 1.0, 0.0],
            projection: Projection::Perspective {
                fov_y_deg: FOV_Y_DEG as f32,
            },
        };
    }
    let height = if shot.scale {
        framed.max(scale_height * 1.1)
    } else {
        framed
    };
    let right = if shot.scale {
        reach + gap + (0.5 * gap).max(0.05)
    } else {
        reach
    };
    let center_x = (right - reach) * 0.5;
    let half_width = f64::midpoint(right, reach);
    let half_fov = (FOV_Y_DEG.to_radians() * 0.5).tan();
    let aspect = shot.aspect.max(0.1);
    let distance = (height * 0.5 / half_fov).max(half_width / (half_fov * aspect)) * 1.12 + reach;
    let target = Vec3::new(center_x, height * 0.5, 0.0);
    #[allow(clippy::cast_possible_truncation)]
    match shot.view {
        View::Side | View::ThreeQuarter => {
            let direction = if shot.view == View::Side {
                Vec3::new(0.0, 0.04, 1.0)
            } else {
                Vec3::new(0.7, 0.3, 0.7)
            }
            .normalize_or(Vec3::Z);
            Framing {
                eye: (target + direction * distance).to_f32(),
                target: target.to_f32(),
                up: [0.0, 1.0, 0.0],
                projection: Projection::Perspective {
                    fov_y_deg: FOV_Y_DEG as f32,
                },
            }
        }
        View::Top => Framing {
            eye: [center_x as f32, (height + 50.0) as f32, 0.0],
            target: [center_x as f32, 0.0, 0.0],
            up: [0.0, 0.0, -1.0],
            projection: Projection::Orthographic {
                half_height: (half_width / aspect).max(reach * 1.1) as f32,
            },
        },
    }
}

/// A picture made of tiles: one plant each, with captions.
#[derive(Debug, Clone)]
pub struct Sheet {
    pub width: u32,
    pub height: u32,
    /// The sheet's title, drawn across its top; empty for none.
    pub title: String,
    pub tiles: Vec<Tile>,
    /// Row headings, at their top-left corners in pixels.
    pub labels: Vec<Label>,
}

/// One plant's place on a sheet.
#[derive(Debug, Clone)]
pub struct Tile {
    pub shot: Shot,
    /// Left, top, width and height in pixels.
    pub rect: [u32; 4],
    /// Drawn under the tile.
    pub caption: String,
}

/// Text placed on a sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub text: String,
    pub x: u32,
    pub y: u32,
    /// Font size, pixels.
    pub size: f32,
}

/// Room for the title above the first row, pixels.
pub const TITLE_BAND: u32 = 56;
/// Room for a row's heading above its tiles, and for captions under them.
pub const ROW_BAND: u32 = 30;
pub const CAPTION_BAND: u32 = 26;
/// Gap between tiles.
pub const GAP: u32 = 12;
/// A review sheet's tiles: 380 by 460 pixels.
pub const TILE: (u32, u32) = (380, 460);
/// Most tiles in a review sheet's row.
pub const ROW_TILES: usize = 6;

/// A sheet of one picture filling it: a thumbnail.
#[must_use]
pub fn single(shot: Shot, width: u32, height: u32) -> Sheet {
    let mut shot = shot;
    #[allow(clippy::cast_precision_loss)]
    {
        shot.aspect = f64::from(width) / f64::from(height);
    }
    Sheet {
        width,
        height,
        title: String::new(),
        tiles: vec![Tile {
            shot,
            rect: [0, 0, width, height],
            caption: String::new(),
        }],
        labels: Vec::new(),
    }
}

/// Mid-month days of the year: January, March, May, July, September and
/// November.
pub const YEAR_DAYS: [(f64, &str); 6] = [
    (15.0, "Jan"),
    (74.0, "Mar"),
    (135.0, "May"),
    (196.0, "Jul"),
    (258.0, "Sep"),
    (319.0, "Nov"),
];

/// A species' review sheet, from `base` (its look, quality, seed and
/// conditions):
/// - **ages:** each keyframe of its growth, side view, all at the oldest
///   plant's scale;
/// - **mature:** the oldest plant from the side, the south-east and above,
///   and a close-up of its crown;
/// - **year:** the oldest plant at six times of the year, when any of its
///   organs has seasons.
///
/// # Errors
///
/// When the species does not load or grow.
#[allow(clippy::too_many_lines)]
pub fn review_sheet(base: &Shot, library: &Library) -> Result<Sheet, String> {
    let spec = load_spec(&base.species, library)?;
    let mut ages = spec.growth.keyframes.clone();
    ages.sort_by(f64::total_cmp);
    ages.dedup();
    let oldest = ages.last().copied().unwrap_or(1.0);
    // The mature plant's height sets every age tile's scale.
    let mature = build(
        &Shot {
            age: Some(oldest),
            ..base.clone()
        },
        library,
    )?;
    let height = mature.facts.height_m;
    let seasons = spec
        .appearance
        .organs
        .values()
        .any(|look| look.season.is_some());

    let (tw, th) = TILE;
    #[allow(clippy::cast_precision_loss)]
    let aspect = f64::from(tw) / f64::from(th);
    let mut rows: Vec<(String, Vec<(Shot, String)>)> = Vec::new();
    let mut age_row: Vec<(Shot, String)> = ages
        .iter()
        .map(|&age| {
            (
                Shot {
                    age: Some(age),
                    view: View::Side,
                    aspect,
                    frame_height: Some(height),
                    ..base.clone()
                },
                format!("{age} years"),
            )
        })
        .collect();
    // At most a row's worth, keeping the youngest and the oldest.
    while age_row.len() > ROW_TILES {
        age_row.remove(age_row.len() / 2);
    }
    rows.push(("Ages, side view, one scale".into(), age_row));
    let crown = [0.0, height * 0.7, 0.0];
    let mature_row = vec![
        (View::Side, None, "side"),
        (View::ThreeQuarter, None, "from the south-east"),
        (View::Top, None, "from above"),
        (
            View::ThreeQuarter,
            Some((crown, (height * 0.25).max(0.3))),
            "close-up",
        ),
    ]
    .into_iter()
    .map(|(view, focus, caption)| {
        (
            Shot {
                age: Some(oldest),
                view,
                aspect,
                focus,
                ..base.clone()
            },
            caption.to_string(),
        )
    })
    .collect();
    rows.push((format!("Mature, {oldest} years"), mature_row));
    if seasons {
        let year_row = YEAR_DAYS
            .iter()
            .map(|&(day, month)| {
                (
                    Shot {
                        age: Some(oldest),
                        day,
                        view: View::ThreeQuarter,
                        aspect,
                        ..base.clone()
                    },
                    format!("mid {month} (day {day})"),
                )
            })
            .collect();
        rows.push(("The year".into(), year_row));
    }

    #[allow(clippy::cast_possible_truncation)]
    let columns = rows.iter().map(|(_, row)| row.len()).max().unwrap_or(1) as u32;
    let width = GAP + columns * (tw + GAP);
    let mut tiles = Vec::new();
    let mut labels = Vec::new();
    let mut y = TITLE_BAND;
    for (heading, row) in rows {
        labels.push(Label {
            text: heading,
            x: GAP,
            y: y + 4,
            size: 20.0,
        });
        y += ROW_BAND;
        for (column, (shot, caption)) in (0_u32..).zip(row) {
            let x = GAP + column * (tw + GAP);
            labels.push(Label {
                text: caption.clone(),
                x,
                y: y + th + 3,
                size: 16.0,
            });
            tiles.push(Tile {
                shot,
                rect: [x, y, tw, th],
                caption,
            });
        }
        y += th + CAPTION_BAND + GAP;
    }
    let facts = &mature.facts;
    let title = format!(
        "{}   |   {} m tall at {} years   |   {}, seed {}, {} quality, {} look   |   generator revision {}",
        facts.species,
        format_metres(facts.height_m),
        facts.age,
        facts.environment,
        facts.seed,
        facts.quality,
        facts.look,
        facts.generator_revision
    );
    labels.insert(
        0,
        Label {
            text: title.clone(),
            x: GAP,
            y: 14,
            size: 24.0,
        },
    );
    Ok(Sheet {
        width,
        height: y,
        title,
        tiles,
        labels,
    })
}

fn format_metres(m: f64) -> String {
    if m >= 10.0 {
        format!("{m:.0}")
    } else {
        format!("{m:.1}")
    }
}

/// A sheet's sidecar: the picture, its title and each tile's rectangle,
/// caption and facts, as JSON.
#[must_use]
pub fn sheet_sidecar(sheet: &Sheet, picture: &str, facts: &[Facts]) -> String {
    let mut tiles = Vec::new();
    for (tile, facts) in sheet.tiles.iter().zip(facts) {
        let [x, y, w, h] = tile.rect;
        let inner = sidecar(facts, picture, w, h);
        // Indent the tile's facts and add its place.
        let body = inner
            .trim()
            .trim_start_matches('{')
            .trim_end_matches('}')
            .trim_end();
        tiles.push(format!(
            "    {{\n      \"rect\": [{x}, {y}, {w}, {h}],\n      \"caption\": {},{}\n    }}",
            serde_json_string(&tile.caption),
            body.replace("\n  ", "\n      ")
        ));
    }
    format!(
        "{{\n  \"picture\": {},\n  \"width\": {},\n  \"height\": {},\n  \"title\": {},\n  \"tiles\": [\n{}\n  ]\n}}\n",
        serde_json_string(picture),
        sheet.width,
        sheet.height,
        serde_json_string(&sheet.title),
        tiles.join(",\n")
    )
}

/// The sidecar beside a picture: what it shows, as JSON.
#[must_use]
pub fn sidecar(facts: &Facts, picture: &str, width: u32, height: u32) -> String {
    let text = |s: &str| serde_json_string(s);
    format!(
        concat!(
            "{{\n",
            "  \"picture\": {},\n",
            "  \"width\": {},\n",
            "  \"height\": {},\n",
            "  \"species\": {},\n",
            "  \"generator_revision\": {},\n",
            "  \"environment\": {},\n",
            "  \"seed\": {},\n",
            "  \"age\": {},\n",
            "  \"day\": {},\n",
            "  \"level\": {},\n",
            "  \"quality\": {},\n",
            "  \"view\": {},\n",
            "  \"look\": {},\n",
            "  \"height_m\": {:.3},\n",
            "  \"crown_width_m\": {:.3},\n",
            "  \"triangles\": {},\n",
            "  \"cards\": {},\n",
            "  \"on_host\": {}\n",
            "}}\n"
        ),
        text(picture),
        width,
        height,
        text(&facts.species),
        facts.generator_revision,
        text(&facts.environment),
        facts.seed,
        facts.age,
        facts.day,
        facts.level,
        text(&facts.quality),
        text(facts.view),
        text(facts.look),
        facts.height_m,
        facts.crown_width_m,
        facts.triangles,
        facts.cards,
        facts.on_host,
    )
}

/// A JSON string literal.
fn serde_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The size of a template layer's side, texels.
pub const LAYER_SIZE: usize = TEMPLATE_SIZE;

#[cfg(test)]
mod tests {
    use super::*;

    fn maple() -> Scene {
        let mut shot = Shot::thumbnail("acer-macrophyllum");
        shot.age = Some(6.0);
        shot.quality = quality::DRAFT;
        build(&shot, Library::builtin()).expect("the maple grows")
    }

    #[test]
    fn a_scene_has_wood_cards_and_a_layer_per_template() {
        let scene = maple();
        assert!(!scene.wood.is_empty());
        assert!(!scene.cards.is_empty());
        assert_eq!(scene.cards.darkening.len(), scene.cards.positions.len());
        assert_eq!(scene.template_accents.len(), scene.templates.layers);
        assert!(scene.templates.layers <= MAX_TEMPLATES);
        assert_eq!(
            scene.templates.rgba.len(),
            scene.templates.layers * LAYER_SIZE * LAYER_SIZE * 4
        );
        // Every card names a layer that exists, plus one.
        #[allow(clippy::cast_precision_loss)]
        let layers = scene.templates.layers as f32;
        assert!(
            scene
                .cards
                .colors
                .iter()
                .all(|c| c[3] >= 1.0 && c[3] <= layers)
        );
        assert_eq!(scene.facts.species, "acer-macrophyllum");
        assert!(scene.facts.height_m > 0.5);
    }

    #[test]
    fn the_card_blend_is_the_generators_albedo() {
        let spec = Library::builtin().spec("acer-macrophyllum").expect("maple");
        let quality = quality::DRAFT;
        let mut request = Request::typical(&spec, Library::builtin(), &quality);
        request.age = 6.0;
        let drawing = drawing::draw(&request).expect("draws");
        let templates = &drawing.templates;
        let colour = [0.08_f32, 0.2, 0.05];
        for template in 0..templates.templates.len() {
            let base = templates.templates[template].accent_colour;
            let dark = darkening(templates, template, colour);
            let accent: [f32; 3] = std::array::from_fn(|c| base[c] * dark);
            for (brightness, weight) in [(1.0_f32, 0.0_f32), (1.3, 0.4), (0.7, 1.0)] {
                let texel = Texel {
                    coverage: 1.0,
                    brightness,
                    accent: weight,
                };
                let want = templates.albedo(template, colour.map(f64::from), texel);
                let got: [f64; 3] = std::array::from_fn(|c| {
                    let (c0, a) = (f64::from(colour[c]), f64::from(accent[c]));
                    (c0 + (a - c0) * f64::from(weight)) * f64::from(brightness)
                });
                for c in 0..3 {
                    assert!((want[c] - got[c]).abs() < 1e-5, "{want:?} {got:?}");
                }
            }
        }
    }

    #[test]
    fn the_same_shot_gives_the_same_scene() {
        let (a, b) = (maple(), maple());
        assert_eq!(a.wood, b.wood);
        assert_eq!(a.cards, b.cards);
        assert_eq!(a.framing, b.framing);
    }

    #[test]
    fn small_plants_get_the_rod_and_tall_ones_the_figure() {
        let (_, tall, gap) = scale(SMALL_PLANT, 1.0);
        assert!((tall - 1.8).abs() < 1e-12 && (gap - 0.8).abs() < 1e-12);
        let (rod, tall, _) = scale(0.9, 0.3);
        assert!((tall - 1.0).abs() < 1e-12);
        assert_eq!(rod.indices.len(), 10 * 5 * 6);
    }

    #[test]
    fn a_review_sheet_has_ages_mature_views_and_captions() {
        let mut base = Shot::thumbnail("acer-macrophyllum");
        base.quality = quality::DRAFT;
        let sheet = review_sheet(&base, Library::builtin()).expect("a sheet");
        assert!(sheet.tiles.len() >= 4 + 2);
        // Age tiles share the mature plant's scale.
        let scaled: Vec<_> = sheet
            .tiles
            .iter()
            .filter_map(|tile| tile.shot.frame_height)
            .collect();
        assert!(scaled.len() >= 2 && scaled.windows(2).all(|w| (w[0] - w[1]).abs() < 1e-9));
        // Tiles lie inside the sheet and never overlap.
        for (i, a) in sheet.tiles.iter().enumerate() {
            let [x, y, w, h] = a.rect;
            assert!(x + w <= sheet.width && y + h <= sheet.height);
            for b in &sheet.tiles[i + 1..] {
                let [bx, by, bw, bh] = b.rect;
                assert!(x + w <= bx || bx + bw <= x || y + h <= by || by + bh <= y);
            }
        }
        assert!(sheet.title.starts_with("acer-macrophyllum"));
    }

    #[test]
    fn cut_cards_keep_the_covered_cells_of_every_card() {
        let spec = Library::builtin().spec("acer-macrophyllum").expect("maple");
        let quality = quality::DRAFT;
        let mut request = Request::typical(&spec, Library::builtin(), &quality);
        request.age = 6.0;
        let drawing = drawing::draw(&request).expect("draws");
        let cut = cut_cards(&drawing.plant, &drawing.templates, 12, usize::MAX);
        let cards = drawing.plant.cards.len();
        // Leaves cover part of their cards: some cells each, not all.
        let triangles = cut.triangle_count();
        assert!(
            triangles > cards * 2,
            "{triangles} triangles for {cards} cards"
        );
        assert!(triangles < cards * 12 * 12 * 2);
        assert_eq!(cut.positions.len(), cut.colors.len());
        assert!(
            cut.indices
                .iter()
                .all(|&i| (i as usize) < cut.positions.len())
        );
    }

    #[test]
    fn the_sidecar_is_json() {
        let text = sidecar(&maple().facts, "a \"b\".png", 512, 512);
        assert!(text.contains("\"picture\": \"a \\\"b\\\".png\""));
        assert!(text.contains("\"species\": \"acer-macrophyllum\""));
    }
}
