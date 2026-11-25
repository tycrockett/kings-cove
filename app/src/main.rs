//! Runs the app directly on this machine's desktop backend (`cargo run -p kings-cove-app`).

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
use nsh_platform_linux as platform;
#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;
#[cfg(target_os = "windows")]
use nsh_platform_windows as platform;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn main() -> impl std::process::Termination {
    env_logger::init();
    platform::run_app(
        platform::AppSource::Rust(platform::Root::of::<kings_cove_app::App>()),
        platform::AppOptions {
            title: "King's Cove".to_owned(),
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            app_id: "dev.kingscove.app".to_owned(),
            // Phone-shaped: the layout is mobile-first.
            size: (400, 820),
            // `KC_DATA_DIR` gives a window its own storage, and so its own
            // seat: several local players on one machine.
            data_dir: std::env::var_os("KC_DATA_DIR").map(std::path::PathBuf::from),
            ..Default::default()
        },
    )
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
compile_error!("Run this binary on macOS, Windows or Linux; use `nsh run --target ios|android` for phones.");
