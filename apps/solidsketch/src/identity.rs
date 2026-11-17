//! What this app is, for whoever has several running (docs/ARCHITECTURE.md §7 septies): its
//! build — the git worktree, branch and commit the build script found, dev builds only — and,
//! on the desktop, its file in the registry of running apps
//! (`solidsketch_control::instances`), which a client of the control channel reads to find the
//! app it means. The title bar shows it as a badge, coloured after the worktree.

use solidsketch_control::instances::Build;

/// What this build was made from: dev builds only (none in a release, nor without git).
pub fn build() -> Option<Build> {
    #[cfg(debug_assertions)]
    {
        let worktree = option_env!("SOLIDSKETCH_GIT_WORKTREE")?;
        Some(Build {
            worktree: worktree.to_owned(),
            branch: option_env!("SOLIDSKETCH_GIT_BRANCH").unwrap_or_default().to_owned(),
            commit: option_env!("SOLIDSKETCH_GIT_COMMIT").unwrap_or_default().to_owned(),
            dirty: option_env!("SOLIDSKETCH_GIT_DIRTY") == Some("1"),
            built: option_env!("SOLIDSKETCH_BUILT").and_then(|s| s.parse().ok()).unwrap_or(0),
        })
    }
    #[cfg(not(debug_assertions))]
    None
}

/// The badge's colour for the build `label`: a hue of its own (a hash of it), the same at
/// every launch, light enough under dark text.
pub fn color(label: &str) -> egui::Color32 {
    // FNV-1a: stable across runs and builds (std's hasher is not).
    let hash = label.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    let hue = (hash % 360) as f32 / 360.0;
    egui::ecolor::Hsva::new(hue, 0.45, 0.85, 1.0).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_keeps_its_colour_and_two_differ() {
        assert_eq!(color("prism-z"), color("prism-z"));
        assert_ne!(color("prism-z"), color("instance-identity"));
    }

    #[test]
    fn a_dev_build_knows_its_worktree() {
        // The tests are a dev build, from a checkout.
        if let Some(b) = build() {
            assert!(!b.commit.is_empty() && std::path::Path::new(&b.worktree).join(".git").exists(), "{b:?}");
        }
    }
}
