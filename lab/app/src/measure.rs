//! The window's measures: a grid on the ground under the plant and a
//! ruler standing at the ground's edge beside it, as tall as the plant,
//! drawn as gizmo lines with stroke-font labels so that snapshots and
//! animations carry them too.

use bevy::camera::visibility::RenderLayers;
use bevy::gizmos::config::{GizmoConfigGroup, GizmoConfigStore};
use bevy::prelude::*;

/// The measures' lines: on the window's layer, a little in front of the
/// ground so they do not flicker against it.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct MeasureGizmos;

/// Put the measures on `layer`.
pub fn configure(store: &mut GizmoConfigStore, layer: usize) {
    let (config, _) = store.config_mut::<MeasureGizmos>();
    config.render_layers = RenderLayers::layer(layer);
    config.depth_bias = -0.0005;
    config.line.width = 1.5;
}

/// The smallest of 1, 2 and 5 times a power of ten that is at least `span`.
#[must_use]
pub fn nice_step(span: f32) -> f32 {
    if !(span > 0.0 && span.is_finite()) {
        return 1.0;
    }
    let power = 10_f32.powf(span.log10().floor());
    let mantissa = span / power;
    let step = if mantissa <= 1.0 {
        1.0
    } else if mantissa <= 2.0 {
        2.0
    } else if mantissa <= 5.0 {
        5.0
    } else {
        10.0
    };
    step * power
}

/// The grid's cell, metres: about twelve cells from the plant to the
/// ground's edge. Every fifth line is bold.
#[must_use]
pub fn grid_step(radius: f32) -> f32 {
    nice_step(radius / 12.0)
}

/// The ruler's labelled step, metres: about eight steps up the plant, with
/// five finer ticks in each.
#[must_use]
pub fn ruler_step(height: f32) -> f32 {
    nice_step(height / 8.0)
}

/// A length on a scale of `step`: metres from a metre up, with the
/// decimals the step needs; below that centimetres, or millimetres on a
/// millimetre scale.
#[must_use]
pub fn length(metres: f32, step: f32) -> String {
    if metres >= 1.0 - 1e-4 || step >= 1.0 {
        // Decimals for a step under a metre: 0.5 needs one, 0.05 two.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let decimals = (-step.log10().floor()).max(0.0) as usize;
        let text = format!("{metres:.decimals$}");
        let text = if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            text
        };
        format!("{text} m")
    } else if step < 0.01 {
        format!("{:.0} mm", metres * 1000.0)
    } else {
        format!("{:.0} cm", metres * 100.0)
    }
}

/// The plant's height, to the precision a reader wants.
#[must_use]
pub fn height_label(metres: f32) -> String {
    if metres < 1.0 {
        format!("{:.1} cm", metres * 100.0)
    } else {
        format!("{metres:.2} m")
    }
}

/// What to draw and how the view sees it.
pub struct View {
    /// The camera's place, rotation, vertical field of view (radians) and
    /// the picture's height in pixels.
    pub eye: Vec3,
    pub rotation: Quat,
    pub fov_y: f32,
    pub pixels: f32,
}

impl View {
    /// The world size of `pixels` at `point`.
    fn size(&self, point: Vec3, pixels: f32) -> f32 {
        let distance = (point - self.eye).length().max(0.01);
        2.0 * distance * (self.fov_y * 0.5).tan() * pixels / self.pixels.max(1.0)
    }

    fn text(
        &self,
        gizmos: &mut Gizmos<MeasureGizmos>,
        at: Vec3,
        text: &str,
        anchor: Vec2,
        color: Color,
    ) {
        let size = self.size(at, 12.0);
        gizmos.text(
            Isometry3d::new(at, self.rotation),
            text,
            size,
            anchor,
            color,
        );
    }
}

const MINOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.22);
const MAJOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.55);
const RULER: Color = Color::srgb(1.0, 0.95, 0.75);
const TOP: Color = Color::srgb(1.0, 0.75, 0.3);

/// The grid on the ground of `radius`, lines clipped to its circle, with
/// the bold lines' distances from the plant on the rim nearest the camera.
pub fn grid(gizmos: &mut Gizmos<MeasureGizmos>, view: &View, radius: f32) {
    let step = grid_step(radius);
    // Whole cells inside the radius; few enough to count in an i32.
    #[allow(clippy::cast_possible_truncation)]
    let cells = (radius / step).floor() as i32;
    let toward = Vec3::new(view.eye.x, 0.0, view.eye.z).normalize_or(Vec3::Z);
    for i in -cells..=cells {
        #[allow(clippy::cast_precision_loss)]
        let along = i as f32 * step;
        let half = (radius * radius - along * along).max(0.0).sqrt();
        let bold = i % 5 == 0;
        let color = if bold { MAJOR } else { MINOR };
        let y = 0.002;
        let x_line = (Vec3::new(along, y, -half), Vec3::new(along, y, half));
        let z_line = (Vec3::new(-half, y, along), Vec3::new(half, y, along));
        gizmos.line(x_line.0, x_line.1, color);
        gizmos.line(z_line.0, z_line.1, color);
        if bold && i != 0 {
            let label = length(along.abs(), step);
            for (a, b) in [x_line, z_line] {
                let end = if a.dot(toward) > b.dot(toward) { a } else { b };
                let at = end + toward * view.size(end, 10.0);
                view.text(gizmos, at, &label, Vec2::ZERO, MAJOR);
            }
        }
    }
}

/// How tall the ruler for a plant `height` tall stands: the next labelled
/// step above it.
#[must_use]
pub fn ruler_top(height: f32) -> f32 {
    let step = ruler_step(height);
    (height / step).ceil().max(1.0) * step
}

/// The direction on the ground to the camera's right.
#[must_use]
pub fn side(view: &View) -> Vec3 {
    (view.rotation * Vec3::X).with_y(0.0).normalize_or(Vec3::X)
}

/// The ruler `at` metres to the camera's right of the plant, as tall as
/// the next labelled step above `height`, with the plant's height marked by
/// a line across to the plant's axis and labelled over it.
pub fn ruler(gizmos: &mut Gizmos<MeasureGizmos>, view: &View, height: f32, at: f32) {
    let step = ruler_step(height);
    let side = side(view);
    let base = side * at;
    let top = ruler_top(height);
    gizmos.line(base, base + Vec3::Y * top, RULER);
    // Ticks point back toward the plant; labels sit outside.
    let inward = -side;
    // At most a few hundred ticks.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let ticks = (top / step * 5.0).round() as u32;
    for tick in 0..=ticks {
        #[allow(clippy::cast_precision_loss)]
        let y = tick as f32 * step / 5.0;
        let at = base + Vec3::Y * y;
        let labelled = tick % 5 == 0;
        let long = view.size(at, if labelled { 14.0 } else { 7.0 });
        gizmos.line(at, at + inward * long, RULER);
        if labelled {
            let label_at = at + side * view.size(at, 6.0);
            // The lowest label stands on the ground, not half in it.
            let anchor = Vec2::new(-0.5, if tick == 0 { -0.5 } else { 0.0 });
            view.text(gizmos, label_at, &length(y, step), anchor, RULER);
        }
    }
    // The plant's height: across from the ruler to the plant's axis,
    // labelled over the line just inside the ruler.
    let mark = base + Vec3::Y * height;
    gizmos.line(mark, Vec3::Y * height, TOP);
    let label_at = mark + inward * view.size(mark, 18.0) + Vec3::Y * view.size(mark, 4.0);
    view.text(
        gizmos,
        label_at,
        &height_label(height),
        Vec2::new(0.5, -0.5),
        TOP,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_are_one_two_or_five_times_a_power_of_ten() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-5 * b.max(1.0);
        assert!(close(nice_step(0.17), 0.2));
        assert!(close(nice_step(1.5), 2.0));
        assert!(close(nice_step(3.0), 5.0));
        assert!(close(nice_step(7.0), 10.0));
        assert!(close(nice_step(10.0), 10.0));
        assert!(close(nice_step(0.004), 0.005));
        assert!(close(nice_step(0.0), 1.0));
        // A 24 m maple: 5 m steps; an 80 cm fern: 10 cm steps.
        assert!(close(ruler_step(24.0), 5.0));
        assert!(close(ruler_step(0.8), 0.1));
        // An 18 m ground: 2 m cells; a 2 m ground: 20 cm cells.
        assert!(close(grid_step(18.0), 2.0));
        assert!(close(grid_step(2.0), 0.2));
    }

    #[test]
    fn lengths_read_in_the_unit_of_their_scale() {
        assert_eq!(length(10.0, 5.0), "10 m");
        assert_eq!(length(0.3, 0.1), "30 cm");
        assert_eq!(length(0.015, 0.005), "15 mm");
        assert_eq!(length(1.0, 0.2), "1 m");
        assert_eq!(length(1.5, 0.5), "1.5 m");
        assert_eq!(length(2.25, 0.05), "2.25 m");
        assert_eq!(height_label(24.054), "24.05 m");
        assert_eq!(height_label(0.805), "80.5 cm");
    }
}
