//! How much memory this process holds and how much it may still take, so
//! work done many at once (a set of variants, [`crate::api::Variants`])
//! runs no more at once than fit.
//!
//! A large old tree takes over a gibibyte while it is drawn (an old
//! Douglas-fir's heap peaked at 1.6 GiB, 2026-11-08), and the set grew as
//! many as there are cores at once: a machine of many cores ran out of
//! memory long before it ran out of cores. The set grows its first plant
//! alone, takes what that plant added to the process's peak, and grows no
//! more at once than that, with a margin for larger seeds, fits in what
//! was free when it began.
//!
//! The figures come from Linux's `/proc` and, where the process runs in a
//! cgroup with a limit (a CI runner's slice, a container), from the
//! cgroup's `memory.max` and `memory.current`, each up its tree. Elsewhere
//! nothing is known, and work runs as many at once as asked.

use std::path::{Path, PathBuf};

/// How much a plant after the first may hold beside the first one's
/// measure: a bigleaf maple's largest of ten seeds drew 2.0 times the
/// bytes of its smallest, its first 1.6 times.
pub const MARGIN: f64 = 1.5;

/// Bytes this process may still take: the least of the system's
/// available memory and, up the process's cgroup, each limit less what
/// the cgroup holds. `None` where the system does not say.
#[must_use]
pub fn free() -> Option<u64> {
    let system = meminfo_available(&std::fs::read_to_string("/proc/meminfo").ok()?);
    let cgroup = std::fs::read_to_string("/proc/self/cgroup")
        .ok()
        .and_then(|text| cgroup_free(Path::new("/sys/fs/cgroup"), &text));
    match (system, cgroup) {
        (Some(system), Some(cgroup)) => Some(system.min(cgroup)),
        (system, cgroup) => system.or(cgroup),
    }
}

/// This process's resident bytes now, and the most it has held
/// (`VmRSS`, `VmHWM`). `None` where the system does not say.
#[must_use]
pub fn resident() -> Option<(u64, u64)> {
    status_resident(&std::fs::read_to_string("/proc/self/status").ok()?)
}

/// How many of `wanted` at once fit in `free` bytes when one takes
/// `each`, with [`MARGIN`]: at least one, at most `wanted`.
#[must_use]
pub fn at_once(wanted: usize, free: u64, each: u64) -> usize {
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let fit = (free as f64 / (each.max(1) as f64 * MARGIN)).floor() as usize;
    fit.clamp(1, wanted.max(1))
}

/// A `/proc/meminfo` line's kibibytes as bytes.
fn kib_field(text: &str, name: &str) -> Option<u64> {
    text.lines().find_map(|line| {
        let rest = line.strip_prefix(name)?.strip_prefix(':')?;
        let kib: u64 = rest.trim().trim_end_matches("kB").trim().parse().ok()?;
        kib.checked_mul(1024)
    })
}

fn meminfo_available(text: &str) -> Option<u64> {
    kib_field(text, "MemAvailable")
}

fn status_resident(text: &str) -> Option<(u64, u64)> {
    Some((kib_field(text, "VmRSS")?, kib_field(text, "VmHWM")?))
}

/// What the cgroup v2 hierarchy under `root` leaves this process, given
/// `/proc/self/cgroup`'s `text`: the least, from the process's own group
/// up to the root, of each `memory.max` less its `memory.current`.
fn cgroup_free(root: &Path, text: &str) -> Option<u64> {
    let relative = text.lines().find_map(|line| line.strip_prefix("0::"))?;
    let mut group: PathBuf = root.join(relative.trim().trim_start_matches('/'));
    let mut least: Option<u64> = None;
    loop {
        let read = |name: &str| std::fs::read_to_string(group.join(name)).ok();
        if let (Some(max), Some(current)) = (read("memory.max"), read("memory.current"))
            && let (Ok(max), Ok(current)) =
                (max.trim().parse::<u64>(), current.trim().parse::<u64>())
        {
            let left = max.saturating_sub(current);
            least = Some(least.map_or(left, |least| least.min(left)));
        }
        if group == root || !group.pop() || !group.starts_with(root) {
            break;
        }
    }
    least
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_proc_fields() {
        let meminfo =
            "MemTotal:       16000000 kB\nMemFree:  100 kB\nMemAvailable:    8000000 kB\n";
        assert_eq!(meminfo_available(meminfo), Some(8_000_000 * 1024));
        let status = "Name:\tplantc\nVmHWM:\t 2000 kB\nVmRSS:\t 1500 kB\n";
        assert_eq!(status_resident(status), Some((1500 * 1024, 2000 * 1024)));
        assert_eq!(meminfo_available("MemTotal: 1 kB\n"), None);
    }

    #[test]
    fn as_many_at_once_as_fit() {
        let gib = 1 << 30;
        // 16 cores, 1.6 GiB a plant: 24 GiB free fits 10, 2 GiB one.
        assert_eq!(at_once(16, 24 * gib, gib * 8 / 5), 10);
        assert_eq!(at_once(16, 2 * gib, gib * 8 / 5), 1);
        assert_eq!(at_once(4, 100 * gib, gib), 4);
        assert_eq!(at_once(4, 0, gib), 1);
    }

    #[test]
    fn cgroup_limits_up_the_tree() {
        let root = std::env::temp_dir().join(format!("plantgen-cgroup-{}", std::process::id()));
        let leaf = root.join("system.slice/runner.service");
        std::fs::create_dir_all(&leaf).unwrap();
        let write = |path: &Path, max: &str, current: &str| {
            std::fs::write(path.join("memory.max"), max).unwrap();
            std::fs::write(path.join("memory.current"), current).unwrap();
        };
        write(&leaf, "max\n", "500\n");
        write(&root.join("system.slice"), "10000\n", "4000\n");
        assert_eq!(
            cgroup_free(&root, "0::/system.slice/runner.service\n"),
            Some(6000)
        );
        write(&leaf, "3000\n", "500\n");
        assert_eq!(
            cgroup_free(&root, "0::/system.slice/runner.service\n"),
            Some(2500)
        );
        assert_eq!(cgroup_free(&root, "1:cpu:/x\n"), None);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
