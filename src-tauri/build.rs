//! Embed package-time cloud defaults (from `gen/release-config.json`).
//! That file is produced by `scripts/inject-release-config.mjs` and is gitignored.
//! Dev builds without inject get empty strings → runtime falls back to local preview URL.

use std::fs;
use std::path::Path;

fn main() {
    let gen = Path::new("gen/release-config.json");
    println!("cargo:rerun-if-changed=gen/release-config.json");

    let (base_url, extra_hosts) = if gen.is_file() {
        match fs::read_to_string(gen) {
            Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(v) => {
                    let base = v
                        .get("cloudBaseUrl")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    let hosts = v
                        .get("cloudAllowedHosts")
                        .and_then(|x| x.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|h| h.as_str().map(|s| s.trim().to_string()))
                                .filter(|s| !s.is_empty())
                                .collect::<Vec<_>>()
                                .join(",")
                        })
                        .unwrap_or_default();
                    (base, hosts)
                }
                Err(_) => (String::new(), String::new()),
            },
            Err(_) => (String::new(), String::new()),
        }
    } else {
        (String::new(), String::new())
    };

    // Escape is unnecessary for rustc-env if we avoid newlines; strip them defensively.
    let base_url = base_url.replace(['\n', '\r'], "");
    let extra_hosts = extra_hosts.replace(['\n', '\r'], "");

    println!("cargo:rustc-env=CHATGPT_TOOLS_CLOUD_BASE_URL={base_url}");
    println!("cargo:rustc-env=CHATGPT_TOOLS_CLOUD_EXTRA_HOSTS={extra_hosts}");

    tauri_build::build()
}
