//! What the webview may ask of the app.

#![allow(
    clippy::needless_pass_by_value,
    reason = "Tauri hands command arguments over by value"
)]

use std::sync::Arc;

use tauri::State;
use wanderer_core::problem::Problem;
use wanderer_core::supervisor;

use crate::core::{Account, Core, Locations, Readiness, Settings};

type Shared<'a> = State<'a, Arc<Core>>;

#[tauri::command]
pub(crate) fn readiness(core: Shared<'_>) -> Readiness {
    core.readiness()
}

/// Tries again to get ready, after the person started the service, say.
#[tauri::command]
pub(crate) fn retry(core: Shared<'_>) {
    if core.readiness() != Readiness::Yes {
        core.start();
    }
}

#[tauri::command]
pub(crate) async fn state(core: Shared<'_>) -> Result<supervisor::State, ()> {
    Ok(core.state().await)
}

#[tauri::command]
pub(crate) async fn settings(core: Shared<'_>) -> Result<Settings, ()> {
    Ok(core.settings().await)
}

#[tauri::command]
pub(crate) async fn save_settings(core: Shared<'_>, settings: Settings) -> Result<(), ()> {
    core.save_settings(settings).await;
    Ok(())
}

#[tauri::command]
pub(crate) async fn connect(core: Shared<'_>) -> Result<(), String> {
    core.connect().await
}

#[tauri::command]
pub(crate) async fn disconnect(core: Shared<'_>) -> Result<(), ()> {
    core.disconnect().await;
    Ok(())
}

/// The webview signed in with `address`.
#[tauri::command]
pub(crate) async fn signed_in(core: Shared<'_>, address: String) -> Result<(), String> {
    let address = address.parse().map_err(|_| "not an address")?;
    let core = Arc::clone(&core);
    // Waits for the core to be ready; the webview need not.
    tauri::async_runtime::spawn(async move { core.signed_in(address).await });
    Ok(())
}

#[tauri::command]
pub(crate) async fn signed_out(core: Shared<'_>) -> Result<(), ()> {
    core.signed_out().await;
    Ok(())
}

/// The webview's answer to a `wallet:sign` request.
#[tauri::command]
pub(crate) fn wallet_signed(
    core: Shared<'_>,
    id: u64,
    signature: Option<String>,
    error: Option<String>,
) {
    let answer = signature.ok_or_else(|| error.unwrap_or_else(|| "no signature".into()));
    core.signatures.answer(id, answer);
}

#[tauri::command]
pub(crate) async fn account(core: Shared<'_>) -> Result<Account, String> {
    core.account().await
}

#[tauri::command]
pub(crate) async fn buy(
    core: Shared<'_>,
    payment: crate::core::PaymentChoice,
    slots: u8,
) -> Result<u64, Problem> {
    core.buy(payment, slots).await
}

#[tauri::command]
pub(crate) async fn add_slots_quote(
    core: Shared<'_>,
    token: u64,
    extra: u8,
) -> Result<crate::core::Quote, Problem> {
    core.add_slots_quote(token, extra).await
}

#[tauri::command]
pub(crate) async fn add_slots(core: Shared<'_>, token: u64, extra: u8) -> Result<(), Problem> {
    core.add_slots(token, extra).await
}

#[tauri::command]
pub(crate) async fn faucet(core: Shared<'_>) -> Result<(), Problem> {
    core.faucet().await
}

#[tauri::command]
pub(crate) async fn activate(core: Shared<'_>, token: u64) -> Result<(), Problem> {
    core.activate(token).await
}

#[tauri::command]
pub(crate) async fn locations(core: Shared<'_>) -> Result<Locations, String> {
    core.locations().await
}

/// Puts the tray menu in the webview's language.
#[cfg(desktop)]
#[tauri::command]
pub(crate) fn tray_labels(
    items: State<'_, crate::tray::TrayItems<tauri::Wry>>,
    show: String,
    quit: String,
) {
    items.relabel(&show, &quit);
}

/// A phone has no tray.
#[cfg(mobile)]
#[tauri::command]
#[allow(clippy::needless_pass_by_value, reason = "the command's arguments")]
pub(crate) fn tray_labels(show: String, quit: String) {
    let _ = (show, quit);
}

/// The app's recent log, and the daemon's state, to copy into a report.
#[tauri::command]
pub(crate) async fn diagnostics(
    core: Shared<'_>,
    book: State<'_, crate::logbook::Logbook>,
) -> Result<String, ()> {
    let mut report = format!(
        "Wanderer {}\nreadiness: {:?}\nstate: {:?}\n\n",
        env!("CARGO_PKG_VERSION"),
        core.readiness(),
        core.state().await
    );
    report.push_str(&book.lines().join("\n"));
    Ok(report)
}
