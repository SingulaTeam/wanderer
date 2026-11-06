//! The tray icon, and the window that hides into it.
//!
//! Closing the window must not end the protection: the session lives in this
//! process, and a VPN that quits when its window closes leaves people
//! unprotected without noticing — or, with the guard up, offline. So the
//! window hides; leaving is "Disconnect and quit" in the tray, which takes
//! the tunnel and the guard down on the way out (`RunEvent::ExitRequested`).
//! Launching the app again shows the hidden window, for desktops that draw
//! no tray.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager as _, Runtime, WindowEvent};

const SHOW: &str = "show";
const QUIT: &str = "quit";

/// The tray's menu items, kept so the webview can put them in its language.
pub(crate) struct TrayItems<R: Runtime> {
    show: MenuItem<R>,
    quit: MenuItem<R>,
}

impl<R: Runtime> TrayItems<R> {
    /// Relabels the menu, in the language the person chose.
    pub(crate) fn relabel(&self, show: &str, quit: &str) {
        let _ = self.show.set_text(show);
        let _ = self.quit.set_text(quit);
    }
}

/// Adds the tray icon and makes closing the main window hide it.
pub(crate) fn install(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, SHOW, "Open Wanderer", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT, "Disconnect and quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Wanderer")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            SHOW => show_main(app),
            QUIT => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    app.manage(TrayItems { show, quit });

    if let Some(window) = app.get_webview_window("main") {
        let hide = window.clone();
        window.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = hide.hide();
            }
        });
    }
    Ok(())
}

/// Brings the main window back.
pub(crate) fn show_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
