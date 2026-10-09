//! Space colonization's per-point work, narrowed in 32-bit floats with a
//! safety margin so that 64-bit decides it as it would alone.
//!
//! For each attraction point a narrowing says whether the plant certainly
//! consumes it, certainly leaves it, or may (then the 64-bit test decides),
//! and lists the buds that may take it: every bud that may perceive it and
//! lies no farther than the nearest bud that certainly perceives it, plus
//! twice the margin. The 64-bit decision among them is the one the full
//! search makes, because the bud that takes the point is always among them.
//! A GPU narrows many points at once (`gpu.rs`, feature `gpu`); [`on_cpu`]
//! does the same in Rust, for tests and for machines without one. Growth is
//! the same bit for bit whichever narrows, or none.

/// Buds kept per point; a point with more goes to the full search.
pub const CANDIDATES: usize = 8;
/// Values per point in a narrowing: status, how many buds, the buds.
pub const STRIDE: usize = 2 + CANDIDATES;
/// Status: the plant certainly consumes the point.
pub const CONSUMED: u32 = 1;
/// Status: the plant certainly leaves the point. Status 0: it may.
pub const LEFT: u32 = 2;

/// A grid as the CPU keeps it: cells per axis (x, y, z) and where each
/// cell's items start, cell `(y · z_cells + z) · x_cells + x` from the
/// grid's low corner.
#[derive(Debug, Clone, Default)]
pub struct Cells {
    pub dims: [i32; 3],
    pub starts: Vec<u32>,
}

/// The margins and limits of a narrowing, in 32-bit.
#[derive(Debug, Clone, Copy, Default)]
pub struct Settings {
    pub kill_sq: f32,
    pub influence_sq: f32,
    pub cone: f32,
    /// How far a 32-bit squared distance may stray from the 64-bit one.
    pub margin_sq: f32,
    /// How far a 32-bit facing may stray, at distances from `near_sq` up.
    pub margin_facing: f32,
    /// Below this squared distance a facing is not trusted.
    pub near_sq: f32,
}

impl Settings {
    /// Margins for points, plant parts and buds no farther than `extent`
    /// metres from the origin on any axis, and reaches (`kill`,
    /// `influence`) up to `reach`: four times the worst rounding of a
    /// difference of two such points, its square and the facing.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn new(kill_sq: f64, influence_sq: f64, cone: f64, extent: f64, reach: f64) -> Self {
        let unit = f64::from(f32::EPSILON) * 0.5;
        // A coordinate of a difference: both ends rounded, then the
        // subtraction.
        let coordinate = 2.0 * extent * unit + reach * unit;
        let root3 = 3.0_f64.sqrt();
        let margin_sq =
            4.0 * (2.0 * root3 * reach * coordinate + 3.0 * reach * reach * unit) + 1e-12;
        let near = 0.05_f64;
        let margin_facing = 4.0 * (root3 * coordinate / near + 8.0 * unit) + 1e-7;
        Self {
            kill_sq: kill_sq as f32,
            influence_sq: influence_sq as f32,
            cone: cone as f32,
            margin_sq: margin_sq as f32,
            margin_facing: margin_facing as f32,
            near_sq: (near * near) as f32,
        }
    }
}

/// The work of one narrowing: attraction points, each with its cell in the
/// plant's grid and in the buds' grid (cells at least the influence wide),
/// and both grids' items: plant points, and buds as position then heading.
#[derive(Debug, Clone, Default)]
pub struct Job {
    pub points: Vec<[f32; 4]>,
    pub plant_keys: Vec<[i32; 4]>,
    pub apex_keys: Vec<[i32; 4]>,
    pub plant: Cells,
    pub plant_items: Vec<[f32; 4]>,
    pub apices: Cells,
    pub apex_items: Vec<[f32; 4]>,
    pub settings: Settings,
}

/// The cell's index, if the grid holds it.
fn cell(key: [i32; 3], dims: [i32; 3]) -> Option<usize> {
    let [x, y, z] = key;
    if x < 0 || y < 0 || z < 0 || x >= dims[0] || y >= dims[1] || z >= dims[2] {
        return None;
    }
    usize::try_from((y * dims[2] + z) * dims[0] + x).ok()
}

/// The items in the 27 cells round `key`.
fn near<'a, T>(
    key: [i32; 4],
    grid: &'a Cells,
    items: &'a [T],
    per: usize,
) -> impl Iterator<Item = (usize, &'a [T])> + 'a {
    (-1..=1).flat_map(move |dz| {
        (-1..=1).flat_map(move |dy| {
            (-1..=1).flat_map(move |dx| {
                cell([key[0] + dx, key[1] + dy, key[2] + dz], grid.dims)
                    .map(|at| grid.starts[at] as usize..grid.starts[at + 1] as usize)
                    .into_iter()
                    .flatten()
                    .map(move |item| (item, &items[item * per..item * per + per]))
            })
        })
    })
}

fn sub(a: [f32; 4], b: [f32; 4]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The narrowing done in Rust, as the GPU's shader does it: one walk over
/// the buds near each point, keeping the `CANDIDATES` nearest that may take
/// it and the nearest that certainly does. Those kept no farther than the
/// latter plus twice the margin are the candidates; if a bud was dropped
/// and every kept one is that near, the point is sent to the full search
/// (the count says more than `CANDIDATES`).
#[must_use]
pub fn on_cpu(job: &Job) -> Vec<u32> {
    let settings = job.settings;
    let mut out = vec![0_u32; job.points.len() * STRIDE];
    for (index, point) in job.points.iter().enumerate() {
        let out = &mut out[index * STRIDE..(index + 1) * STRIDE];
        let nearest = near(job.plant_keys[index], &job.plant, &job.plant_items, 1)
            .map(|(_, item)| {
                let d = sub(item[0], *point);
                dot(d, d)
            })
            .fold(f32::MAX, f32::min);
        out[0] = if nearest <= settings.kill_sq - settings.margin_sq {
            CONSUMED
        } else if nearest > settings.kill_sq + settings.margin_sq {
            LEFT
        } else {
            0
        };
        if out[0] == CONSUMED {
            continue;
        }
        let mut sure = f32::MAX;
        let mut kept: Vec<(f32, u32)> = Vec::with_capacity(CANDIDATES + 1);
        let mut dropped = false;
        for (bud_index, bud) in near(job.apex_keys[index], &job.apices, &job.apex_items, 2) {
            let d = sub(*point, bud[0]);
            let d2 = dot(d, d);
            if d2 > settings.influence_sq + settings.margin_sq {
                continue;
            }
            if d2 >= settings.near_sq {
                let heading = [bud[1][0], bud[1][1], bud[1][2]];
                let facing = dot(heading, d) / d2.sqrt();
                if facing < settings.cone - settings.margin_facing {
                    continue;
                }
                if facing >= settings.cone + settings.margin_facing
                    && d2 <= settings.influence_sq - settings.margin_sq
                {
                    sure = sure.min(d2);
                }
            }
            let at = kept.partition_point(|(kept_d2, _)| *kept_d2 <= d2);
            kept.insert(at, (d2, u32::try_from(bud_index).unwrap_or(u32::MAX)));
            if kept.len() > CANDIDATES {
                kept.pop();
                dropped = true;
            }
        }
        let limit = sure + 2.0 * settings.margin_sq;
        let count = kept.iter().take_while(|(d2, _)| *d2 <= limit).count();
        if dropped && count == kept.len() {
            out[1] = u32::try_from(CANDIDATES + 1).unwrap_or(u32::MAX);
            continue;
        }
        for (slot, (_, bud)) in out[2..].iter_mut().zip(&kept[..count]) {
            *slot = *bud;
        }
        out[1] = u32::try_from(count).unwrap_or(u32::MAX);
    }
    out
}

/// Where growth's light and space spend their time, summed over the
/// process: `PLANTGEN_GPU_TIMES=1` makes `plantc grow` print it.
pub mod times {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;

    static COUNTS: [AtomicU64; 13] = [const { AtomicU64::new(0) }; 13];

    /// Steps that ran light or space, and nanoseconds in them.
    pub(crate) const STEPS: usize = 0;
    pub(crate) const LIGHT_SPACE: usize = 1;
    /// Space colonization's large steps, and nanoseconds in them.
    pub(crate) const LARGE: usize = 2;
    pub(crate) const LARGE_TIME: usize = 3;
    /// Steps narrowed, their points, the points whose consumption 64-bit
    /// decided and those sent to the full search.
    pub(crate) const NARROWED: usize = 4;
    pub(crate) const POINTS: usize = 5;
    pub(crate) const UNSURE: usize = 6;
    pub(crate) const FULL: usize = 7;
    /// Nanoseconds gathering the points (with their cells), assembling the
    /// job, narrowing (on a GPU: uploading, dispatching and reading back)
    /// and deciding.
    pub(crate) const GATHER: usize = 8;
    pub(crate) const ASSEMBLE: usize = 9;
    pub(crate) const NARROW: usize = 10;
    pub(crate) const DECIDE: usize = 11;
    /// Nanoseconds waiting for the GPU once light was done.
    pub(crate) const WAIT: usize = 12;

    pub(crate) fn add(slot: usize, value: usize) {
        COUNTS[slot].fetch_add(value as u64, Ordering::Relaxed);
    }

    /// Add the time since `since` to `slot`, and start again.
    pub(crate) fn lap(slot: usize, since: &mut Instant) {
        let now = Instant::now();
        let nanos = u64::try_from(now.duration_since(*since).as_nanos()).unwrap_or(u64::MAX);
        COUNTS[slot].fetch_add(nanos, Ordering::Relaxed);
        *since = now;
    }

    /// One line: the counts and times so far.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn report() -> String {
        let value = |slot: usize| COUNTS[slot].load(Ordering::Relaxed);
        let seconds = |slot: usize| value(slot) as f64 * 1e-9;
        format!(
            "light and space {:.1} s in {} steps; space colonization's large steps {:.1} s in {}, \
             {} narrowed ({} points; on the CPU {} consumption tests, {} full searches): \
             gathering {:.1} s, assembling {:.1} s, narrowing {:.1} s, waiting for the GPU \
             after light {:.1} s, deciding {:.1} s",
            seconds(LIGHT_SPACE),
            value(STEPS),
            seconds(LARGE_TIME),
            value(LARGE),
            value(NARROWED),
            value(POINTS),
            value(UNSURE),
            value(FULL),
            seconds(GATHER),
            seconds(ASSEMBLE),
            seconds(NARROW),
            seconds(WAIT),
            seconds(DECIDE),
        )
    }
}

/// What narrows a step's points: a GPU, or the same narrowing in Rust.
#[derive(Clone, Copy)]
pub enum Narrower {
    #[cfg(feature = "gpu")]
    Gpu(&'static super::gpu::Gpu),
    /// [`on_cpu`]: with `PLANTGEN_GPU=emulate`, or in [`emulated`].
    Rust,
}

thread_local! {
    static EMULATE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Narrow every step's points in Rust on this thread while `work` runs,
/// however few: tests hold the narrowed path to the full search with it.
pub fn emulated<R>(work: impl FnOnce() -> R) -> R {
    EMULATE.with(|emulate| emulate.set(true));
    let result = work();
    EMULATE.with(|emulate| emulate.set(false));
    result
}

impl Narrower {
    /// Whether a GPU narrows large steps in this process.
    #[must_use]
    pub fn gpu() -> bool {
        #[cfg(feature = "gpu")]
        let gpu = super::gpu::Gpu::get().is_some();
        #[cfg(not(feature = "gpu"))]
        let gpu = false;
        gpu
    }

    /// What narrows a step's points, if anything: a GPU for a `large`
    /// step, where the process has one.
    #[must_use]
    pub fn find(large: bool) -> Option<Self> {
        static ASKED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let asked = *ASKED
            .get_or_init(|| std::env::var("PLANTGEN_GPU").is_ok_and(|wish| wish == "emulate"));
        if asked || EMULATE.with(std::cell::Cell::get) {
            return Some(Self::Rust);
        }
        if !large {
            return None;
        }
        #[cfg(feature = "gpu")]
        if let Some(gpu) = super::gpu::Gpu::get() {
            return Some(Self::Gpu(gpu));
        }
        None
    }

    /// The narrowing of `job`, `STRIDE` values a point; `None` if it
    /// failed and the full search must do. A GPU runs `meanwhile` on this
    /// thread while it works.
    #[must_use]
    pub fn narrow(self, job: &Job, meanwhile: &mut dyn FnMut()) -> Option<Vec<u32>> {
        match self {
            #[cfg(feature = "gpu")]
            Self::Gpu(gpu) => gpu.narrow(job, meanwhile),
            Self::Rust => {
                let _ = meanwhile;
                let mut clock = std::time::Instant::now();
                let narrowed = on_cpu(job);
                times::lap(times::NARROW, &mut clock);
                Some(narrowed)
            }
        }
    }
}
