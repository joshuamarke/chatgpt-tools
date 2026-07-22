//! Loopback-only Chrome DevTools Protocol client + native skin engine path.
//!
//! Hot + cold apply, status/detect, settings, and restore without system Node
//! for the main user path. Import/export/design still fall back to Node CLI.

mod host;
mod http;
mod image;
mod inject;
mod keep;
mod launch;
mod native;
mod package;
mod payload;
mod session;
mod theme;

pub use native::{
    apply_skin_native_opts, delete_skin_native, detect_native, engine_paths_native,
    engine_version_native, get_host_status_native, get_status_native, pause_skin_native,
    resolve_asset_native, restore_skin_native, resume_skin_native, set_app_path_native,
};
pub use package::{export_skin_native, import_skin_native, inspect_skin_native};
