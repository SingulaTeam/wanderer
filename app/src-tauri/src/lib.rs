//! Wanderer, the app: a webview for the person, the core behind it.
//!
//! - [`core`]: starting the core and driving it for the UI.
//! - [`commands`]: what the webview may call.
//! - [`wallet`]: signing through the wallet in the webview.
//! - [`logbook`]: the app's recent log, for the diagnostics screen.
//! - [`oauth`]: Google and Apple sign-in windows.
//! - [`relay`]: the relay to Control Planes' fronts.
//! - [`tray`]: the tray icon, and the window that hides into it (desktop).
//! - [`android`]: the tunnel on Android, through the app's VPN service.

#[cfg(target_os = "android")]
mod android;
mod commands;
mod core;
mod logbook;
mod oauth;
mod relay;
#[cfg(desktop)]
mod tray;
mod wallet;

use std::sync::Arc;

use tauri::{Manager as _, RunEvent};

/// Wallet apps a sign-in may hand off to; any other scheme is refused.
#[cfg(desktop)]
const WALLET_SCHEMES: &[&str] = &[
    "wc",
    "metamask",
    "rainbow",
    "trust",
    "cbwallet",
    "uniswap",
    "zerion",
    "imtoken",
    "tokenpocket",
    "safepal",
];

/// Runs the app.
///
/// # Panics
///
/// If the window or the event loop cannot be created: there is no app then.
#[allow(
    clippy::expect_used,
    reason = "without an event loop there is nothing to report to"
)]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let logbook = logbook::Logbook::default();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "wanderer_app_lib=info,wanderer_core=info".into()),
        )
        .with_ansi(false)
        .with_writer(logbook.clone())
        .init();

    let builder = tauri::Builder::default();
    // Launched again while running: show the window it hid instead.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
        tray::show_main(app);
    }));
    #[cfg(target_os = "android")]
    let builder = builder
        .plugin(android::init())
        // Google and Apple sign-in come back as a `singula://oauth` link.
        .plugin(tauri_plugin_deep_link::init());
    builder
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            commands::readiness,
            commands::retry,
            commands::state,
            commands::settings,
            commands::save_settings,
            commands::connect,
            commands::disconnect,
            commands::signed_in,
            commands::signed_out,
            commands::wallet_signed,
            commands::account,
            commands::buy,
            commands::activate,
            commands::faucet,
            commands::add_slots_quote,
            commands::add_slots,
            commands::locations,
            commands::tray_labels,
            commands::diagnostics,
            oauth::open_popup_window,
        ])
        .manage(logbook)
        .setup(|app| {
            let core = core::Core::new(app.handle().clone());
            core.start();
            app.manage(core);
            oauth::close_popup_on_result(app.handle());
            main_window(app)?;
            #[cfg(desktop)]
            tray::install(app)?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("the app starts")
        .run(|app, event| {
            if let RunEvent::ExitRequested { .. } = event {
                // Leaving the guard up with nothing to take it down would
                // leave the machine offline.
                let core = Arc::clone(&*app.state::<Arc<core::Core>>());
                tauri::async_runtime::block_on(core.shut_down());
            }
        });
}

#[cfg(mobile)]
fn main_window(app: &tauri::App) -> tauri::Result<()> {
    tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("index.html".into()))
        .initialization_script(oauth::MAIN_INIT_SCRIPT)
        .build()?;
    Ok(())
}

#[cfg(desktop)]
fn main_window(app: &tauri::App) -> tauri::Result<()> {
    #[cfg(debug_assertions)]
    let url = tauri::WebviewUrl::External(
        "http://127.0.0.1:1420/"
            .parse()
            .map_err(|_| tauri::Error::InvalidWebviewUrl("the dev server URL"))?,
    );
    #[cfg(not(debug_assertions))]
    let url = tauri::WebviewUrl::App("index.html".into());

    let handle = app.handle().clone();
    tauri::WebviewWindowBuilder::new(app, "main", url)
        .title("Wanderer")
        .inner_size(420.0, 720.0)
        // A fixed, compact window with its own title bar, as VPN clients go.
        // Without system decorations Linux also loses its resize grips, so a
        // resizable window would need hit zones of its own on every edge.
        .resizable(false)
        .maximizable(false)
        .decorations(false)
        .shadow(true)
        .center()
        .initialization_script(oauth::MAIN_INIT_SCRIPT)
        // WebKitGTK cannot open wallet links (wc:, metamask:, …) and shows an
        // error page instead; hand them to the system.
        .on_navigation(move |url| {
            let scheme = url.scheme();
            if matches!(
                scheme,
                "http" | "https" | "tauri" | "ipc" | "about" | "data" | "blob"
            ) {
                return true;
            }
            if WALLET_SCHEMES.contains(&scheme) {
                #[allow(
                    deprecated,
                    reason = "the opener plugin would be one more for one call"
                )]
                let _ = tauri_plugin_shell::ShellExt::shell(&handle).open(url.as_str(), None);
            } else {
                tracing::warn!(scheme, "refused a link");
            }
            false
        })
        .build()?;
    Ok(())
}
