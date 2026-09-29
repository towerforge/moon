//! GPU memory, on machines with a card of their own: NVIDIA through NVML
//! (the library behind `nvidia-smi`, loaded at run time if it is there),
//! AMD through the `amdgpu` driver's files under `/sys/class/drm`. With
//! several cards the readings add up. On Apple Silicon there is nothing to
//! read: the GPU shares the RAM, which is already on the plot.

use std::path::{Path, PathBuf};

use nvml_wrapper::Nvml;

/// Where the readings come from.
pub enum Gpu {
    Nvml(Box<Nvml>),
    /// The `(used, total)` files of each card.
    Sysfs(Vec<(PathBuf, PathBuf)>),
}

impl Gpu {
    /// Whatever answers: NVML first, then the amdgpu files. `None` without
    /// a card to read, which is the case on every Mac.
    pub fn detect() -> Option<Self> {
        if let Ok(nvml) = Nvml::init() {
            if nvml.device_count().is_ok_and(|n| n > 0) {
                return Some(Gpu::Nvml(Box::new(nvml)));
            }
        }
        let cards = amdgpu_cards(Path::new("/sys/class/drm"));
        (!cards.is_empty()).then_some(Gpu::Sysfs(cards))
    }

    /// `(used, total)` bytes over all the cards; `None` if a reading fails.
    pub fn read(&self) -> Option<(u64, u64)> {
        let (mut used, mut total) = (0u64, 0u64);
        match self {
            Gpu::Nvml(nvml) => {
                for i in 0..nvml.device_count().ok()? {
                    let m = nvml.device_by_index(i).ok()?.memory_info().ok()?;
                    used = used.saturating_add(m.used);
                    total = total.saturating_add(m.total);
                }
            }
            Gpu::Sysfs(cards) => {
                for (u, t) in cards {
                    used = used.saturating_add(read_u64(u)?);
                    total = total.saturating_add(read_u64(t)?);
                }
            }
        }
        (total > 0).then_some((used, total))
    }
}

/// The `cardN` entries under `drm` whose driver keeps the VRAM counters
/// (amdgpu does; the connectors, `card0-DP-1`, are skipped).
fn amdgpu_cards(drm: &Path) -> Vec<(PathBuf, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(drm) else {
        return Vec::new();
    };
    let mut cards: Vec<(PathBuf, PathBuf)> = entries
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            name.to_string_lossy()
                .strip_prefix("card")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
        .map(|e| {
            let dev = e.path().join("device");
            (
                dev.join("mem_info_vram_used"),
                dev.join("mem_info_vram_total"),
            )
        })
        .filter(|(u, t)| u.is_file() && t.is_file())
        .collect();
    cards.sort();
    cards
}

fn read_u64(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(drm: &Path, name: &str, used: Option<&str>, total: Option<&str>) {
        let dev = drm.join(name).join("device");
        std::fs::create_dir_all(&dev).unwrap();
        if let Some(u) = used {
            std::fs::write(dev.join("mem_info_vram_used"), u).unwrap();
        }
        if let Some(t) = total {
            std::fs::write(dev.join("mem_info_vram_total"), t).unwrap();
        }
    }

    #[test]
    fn only_the_cards_with_both_counters_count() {
        let dir = tempfile::tempdir().unwrap();
        let drm = dir.path();
        card(drm, "card0", Some("1073741824\n"), Some("8589934592\n"));
        card(drm, "card1", Some("5"), None); // no total: not a vram card
        card(drm, "card0-DP-1", Some("1"), Some("2")); // a connector
        card(drm, "renderD128", Some("1"), Some("2"));
        card(drm, "card2", Some("2147483648"), Some("17179869184"));
        let cards = amdgpu_cards(drm);
        assert_eq!(cards.len(), 2);
        assert!(cards[0].0.starts_with(drm.join("card0")));
        assert!(cards[1].0.starts_with(drm.join("card2")));
        // the readings add up over the cards
        let gpu = Gpu::Sysfs(cards);
        assert_eq!(gpu.read(), Some((3 << 30, 24 << 30)));
        // a counter that stops reading takes the whole reading down
        std::fs::write(drm.join("card2/device/mem_info_vram_used"), "junk").unwrap();
        assert_eq!(gpu.read(), None);
        assert!(amdgpu_cards(&drm.join("nowhere")).is_empty());
    }
}
