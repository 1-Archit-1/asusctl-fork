//! GPU switching via supergfxctl CLI.
//!
//! This module wires the same `GPUPageData` bindings as `setup_gpu.rs` but
//! drives them through `supergfxctl` instead of the asusd firmware-attributes
//! DBus interface. It is used when `supergfxd.service` is detected as running.
//!
//! supergfxctl commands used:
//!   supergfxctl --get              → current mode (e.g. "Hybrid")
//!   supergfxctl --supported        → supported modes (e.g. "Integrated, Hybrid, AsusMuxDgpu")
//!   supergfxctl --pend-action      → pending action (e.g. "No action required" or "Logout")
//!   supergfxctl --pend-mode        → pending target mode if any
//!   supergfxctl -m <Mode>          → set mode

use log::error;
use slint::{ComponentHandle, SharedString, Weak};

use crate::{GPUPageData, MainWindow};

/// Map a supergfxctl mode string to a display label.
fn mode_label(mode: &str) -> Option<&'static str> {
    match mode.trim() {
        "Integrated" => Some("Integrated"),
        "Hybrid" => Some("Hybrid"),
        "AsusMuxDgpu" => Some("Ultimate (MUX)"),
        _ => None,
    }
}

/// Map a display label back to the supergfxctl mode string for `-m`.
fn label_to_supergfx_mode(label: &str) -> Option<&'static str> {
    match label {
        "Integrated" => Some("Integrated"),
        "Hybrid" => Some("Hybrid"),
        "Ultimate (MUX)" => Some("AsusMuxDgpu"),
        _ => None,
    }
}

/// Run a supergfxctl command and return trimmed stdout, or an error string.
fn run_supergfxctl(args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("supergfxctl")
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run supergfxctl: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn get_current_mode() -> Option<String> {
    match run_supergfxctl(&["--get"]) {
        Ok(out) => mode_label(out.trim()).map(|s| s.to_string()),
        Err(e) => {
            error!("setup_gpu_supergfx: --get failed: {e}");
            None
        }
    }
}

fn get_supported_modes() -> Vec<String> {
    match run_supergfxctl(&["--supported"]) {
        Ok(out) => out
            .trim_matches(|c| c == '[' || c == ']')
            .split(',')
            .filter_map(|m| mode_label(m.trim()).map(|s| s.to_string()))
            .collect(),
        Err(e) => {
            error!("setup_gpu_supergfx: --supported failed: {e}");
            vec![]
        }
    }
}

fn set_mode(label: &str) -> Result<(), String> {
    let mode = label_to_supergfx_mode(label)
        .ok_or_else(|| format!("Unknown mode label: {label}"))?;
    run_supergfxctl(&["-m", mode]).map(|_| ())
}

/// Returns true if there is already a pending mode change waiting for logout/reboot.
fn has_pending_change() -> bool {
    match run_supergfxctl(&["--pend-action"]) {
        Ok(out) => out.trim() != "No action required",
        Err(_) => false,
    }
}

/// Returns the pending target mode label if any.
fn get_pending_mode() -> Option<String> {
    match run_supergfxctl(&["--pend-mode"]) {
        Ok(out) => mode_label(out.trim()).map(|s| s.to_string()),
        Err(_) => None,
    }
}

/// Show a toast and auto-dismiss after `secs` seconds.
fn show_timed_toast(handle: &Weak<MainWindow>, msg: SharedString, secs: u64) {
    let toast_handle = handle.clone();
    let msg_clone = msg.clone();
    slint::invoke_from_event_loop(move || {
        if let Some(h) = toast_handle.upgrade() {
            h.invoke_show_toast(msg);
        }
    })
    .ok();

    let dismiss_handle = handle.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
        slint::invoke_from_event_loop(move || {
            if let Some(h) = dismiss_handle.upgrade() {
                h.invoke_clear_toast_if_matches(msg_clone);
            }
        })
        .ok();
    });
}

/// Disable dropdown, call supergfxctl -m, show toast, lock dropdown on success.
fn apply_gpu_mode(handle: Weak<MainWindow>, modes: Vec<String>, index: usize) {
    if let Some(h) = handle.upgrade() {
        h.global::<GPUPageData>().set_gpu_dropdown_enabled(false);
    }

    tokio::task::spawn_blocking(move || {
        let label = match modes.get(index) {
            Some(l) => l.clone(),
            None => {
                error!("setup_gpu_supergfx: invalid mode index {index}");
                handle
                    .upgrade_in_event_loop(|h| {
                        h.global::<GPUPageData>().set_gpu_dropdown_enabled(true);
                    })
                    .ok();
                return;
            }
        };

        let is_mux = label == "Ultimate (MUX)";
        let result = set_mode(&label);

        let toast_msg: SharedString = match &result {
            Ok(_) => {
                if is_mux {
                    SharedString::from(format!(
                        "Switching to {} — reboot required for changes to apply.",
                        label
                    ))
                } else {
                    SharedString::from(format!(
                        "Switching to {} — logout and log back in for changes to take effect.",
                        label
                    ))
                }
            }
            Err(e) => SharedString::from(format!("Failed to set GPU mode: {e}")),
        };

        show_timed_toast(&handle, toast_msg, 8);

        // Refresh current mode and check pending state
        let current = get_current_mode().unwrap_or_else(|| label.clone());
        let pending = has_pending_change();
        let new_index = modes
            .iter()
            .position(|m| *m == current)
            .unwrap_or(index) as i32;

        handle
            .upgrade_in_event_loop(move |h| {
                h.global::<GPUPageData>().set_gpu_mode_index(new_index);
                // Re-enable only on failure. On success supergfxctl won't
                // accept another switch until logout/reboot so keep locked.
                if result.is_err() || !pending {
                    h.global::<GPUPageData>().set_gpu_dropdown_enabled(true);
                }
            })
            .unwrap_or_else(|e| error!("setup_gpu_supergfx: failed to refresh mode: {e:?}"));
    });
}

/// Wire the GPU page using supergfxctl. Called instead of `setup_gpu_page`
/// when supergfxd is detected as running.
pub fn setup_gpu_supergfx_page(ui: &MainWindow) {
    let handle = ui.as_weak();

    tokio::task::spawn_blocking(move || {
        let modes = get_supported_modes();
        let switchable = !modes.is_empty();
        let current = get_current_mode().unwrap_or_default();
        let pending = has_pending_change();
        let pending_mode = get_pending_mode();

        let current_index = modes
            .iter()
            .position(|m| *m == current)
            .unwrap_or(0) as i32;

        let choices: Vec<SharedString> = modes
            .iter()
            .map(|m| SharedString::from(m.as_str()))
            .collect();

        // Build startup toast if there's already a pending change
        let startup_toast: Option<SharedString> = if pending {
            let target = pending_mode.unwrap_or_else(|| "unknown".to_string());
            Some(SharedString::from(format!(
                "Pending GPU switch to {} — logout or reboot to apply.",
                target
            )))
        } else {
            None
        };

        let modes_cb = modes.clone();
        let handle_cb = handle.clone();

        if let Err(e) = handle.upgrade_in_event_loop(move |h| {
            let global = h.global::<GPUPageData>();
            global.set_gpu_modes(choices.as_slice().into());
            global.set_gpu_switchable(switchable);
            // Disable dropdown if a switch is already pending
            global.set_gpu_dropdown_enabled(switchable && !pending);
            global.set_gpu_mode_index(current_index);

            global.on_cb_set_gpu_mode(move |index| {
                apply_gpu_mode(handle_cb.clone(), modes_cb.clone(), index as usize);
            });
        }) {
            error!("setup_gpu_supergfx: upgrade_in_event_loop: {e:?}");
        }

        // Show pending toast after a short delay so the window is visible first
        if let Some(msg) = startup_toast {
            std::thread::sleep(std::time::Duration::from_millis(500));
            show_timed_toast(&handle, msg, 8);
        }
    });
}
