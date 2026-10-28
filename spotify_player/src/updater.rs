//! Updates the running binary from the GitHub releases of spoti. Only release builds have the
//! `self-update` feature, so a build from source is never replaced behind its owner's back.

use anyhow::Result;
use self_update::{backends::github, cargo_crate_version, UpdateStrategy, VersionStatus};

/// Downloads the newest release over the current binary, if there is a newer one. The running
/// app keeps its old code; the new version starts with the next launch.
pub fn update(show_output: bool) -> Result<VersionStatus> {
    Ok(github::Update::configure()
        .repo_owner("Cartograsoda")
        .repo_name("spoti")
        .bin_name("spoti")
        // each target has an archive and its .sha256 file
        .asset_identifier(if cfg!(windows) { ".zip" } else { ".tar.gz" })
        .current_version(cargo_crate_version!())
        .update_strategy(UpdateStrategy::Latest)
        .show_output(show_output)
        .no_confirm(true)
        .build()?
        .update()?)
}

pub fn spawn_background_update() -> Result<()> {
    std::thread::Builder::new()
        .name("self-update".to_string())
        .spawn(|| match update(false) {
            Ok(status) if status.is_updated() => tracing::info!(
                "Updated to v{}, starting with the next launch",
                status.version()
            ),
            Ok(_) => {}
            Err(err) => tracing::warn!("Failed to update: {err:#}"),
        })?;
    Ok(())
}
