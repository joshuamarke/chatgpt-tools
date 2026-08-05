//! Loopback-only Chrome DevTools Protocol client + **single-path** skin engine.
//!
//! All skin ops (apply / status / restore / package / design-wallpaper) run
//! in-process. Host inject assets still load from `engine/runtime/*`.

mod design;
mod host;
pub(crate) mod http;
mod image;
mod inject;
pub mod inspect;
mod keep;
mod launch;
pub(crate) mod library;
pub(crate) mod native;
mod package;
mod payload;
pub(crate) mod session;
mod theme;
#[cfg(windows)]
mod win_native;

pub use design::design_wallpaper_native;
pub use native::{
    apply_skin_native_opts, delete_skin_native, detect_native, engine_paths_native,
    engine_version_native, get_host_status_native, get_status_native, pause_skin_native,
    resolve_asset_native, restart_host_native, restore_skin_native, resume_skin_native,
    set_app_path_native, start_host_native,
};
pub use package::{export_skin_native, import_skin_native, inspect_skin_native};

/// Cloud / package helpers exposed for the cloud module (not GUI-facing).
pub fn native_state_root() -> std::path::PathBuf {
    native::state_root()
}

/// Store package snapshot for environment / detect UIs (Windows; empty-ish elsewhere).
pub fn store_package_status_json() -> serde_json::Value {
    launch::store_package_status_json()
}

pub fn native_safe_skin_id(id: &str) -> String {
    library::safe_skin_id(id)
}

/// Unique skin install root (`%STATE%/library`).
pub fn native_library_dir() -> std::path::PathBuf {
    library::library_dir()
}

pub fn install_skin_to_library(
    src: &std::path::Path,
    id: &str,
    origin: &str,
    extra_meta: serde_json::Value,
) -> Result<std::path::PathBuf, crate::engine::EngineError> {
    library::install_skin_tree(src, id, origin, extra_meta)
}

pub fn read_library_cloud_meta(skin_id: &str) -> Option<serde_json::Value> {
    library::read_library_cloud_meta(skin_id)
}

pub fn library_skin_path(skin_id: &str) -> std::path::PathBuf {
    library::library_skin_dir(skin_id)
}

pub fn native_engine_protocol() -> u32 {
    native::ENGINE_PROTOCOL
}

pub fn extract_zip_package(
    package_path: &std::path::Path,
    tmp_root: &std::path::Path,
) -> Result<Vec<String>, crate::engine::EngineError> {
    package::extract_zip_to_pub(package_path, tmp_root)
}

pub fn resolve_skin_dir_extracted(
    tmp_root: &std::path::Path,
) -> Result<std::path::PathBuf, crate::engine::EngineError> {
    package::resolve_skin_dir_from_extracted_pub(tmp_root)
}

pub fn validate_skin_manifest_pub(
    manifest: &serde_json::Value,
    skin_dir: &std::path::Path,
) -> Result<(), crate::engine::EngineError> {
    package::validate_skin_manifest(manifest, skin_dir)
}
