//! Google and Apple sign-in.
//!
//! On a phone there is no second window: the sign-in page opens in the
//! system browser, told to come back to `singula://oauth` with the result,
//! and the main window hands that link to thirdweb (`src/lib/oauth.ts`).
//! The rest of this is the desktop's way.
//!
//! thirdweb signs in with a provider by opening it in a popup and waiting
//! for the popup to post the result to `window.opener`. A desktop webview
//! cannot open popups — `WebKitGTK` returns `null` from `window.open` — so the
//! main window's `window.open` is replaced: for a sign-in URL it asks the app
//! to open a window of its own and hands thirdweb a stand-in popup object.
//! That window's `window.opener` is replaced in turn: posting to it emits the
//! `oauth_postmessage` event, which the main window turns back into the
//! message thirdweb waits for (`src/lib/oauth.ts`).
//!
//! The sign-in pages are remote, and since Tauri 2.11 a remote page may call
//! an app command only through an app ACL manifest — which, once present,
//! puts every command under the ACL. Emitting a core event needs only
//! `core:event:allow-emit`, granted to that window alone in
//! `capabilities/oauth-popup.json`.

use tauri::{AppHandle, Emitter as _, Listener as _, Manager as _};

/// Injected into the main window before any page code runs.
pub(crate) const MAIN_INIT_SCRIPT: &str = r"
(function () {
    'use strict';
    var _open = window.open;
    window.open = function (url, target, features) {
        var f = (features || '');
        var isOAuthPopup = url && typeof url === 'string' &&
            url.match(/^https?:\/\//) &&
            (f.match(/width|height|popup/i) ||
             url.indexOf('thirdweb') !== -1 ||
             url.indexOf('accounts.google') !== -1 ||
             url.indexOf('appleid.apple') !== -1);
        if (isOAuthPopup && window.__TAURI_INTERNALS__) {
            window.__wandererOauthPopupClosed = false;
            window.__TAURI_INTERNALS__
                .invoke('open_popup_window', { url: String(url) })
                .catch(function (e) { console.error('[wanderer] open_popup_window failed', e); });
            var _fakeBody = { innerHTML: '', style: {} };
            var _fakeDoc = {
                get title() { return ''; },
                set title(_v) {},
                get body() { return _fakeBody; },
                write: function() {},
                writeln: function() {},
                createElement: function(tag) {
                    return { style: {}, setAttribute: function(){}, appendChild: function(){} };
                },
            };
            return {
                get closed() { return !!window.__wandererOauthPopupClosed; },
                focus: function () {},
                blur: function () {},
                close: function () {},
                postMessage: function () {},
                get document() { return _fakeDoc; },
                location: { href: String(url), replace: function(){}, assign: function(){} },
            };
        }
        return _open ? _open.call(window, url, target, features) : null;
    };
})();
";

/// Injected into every page the sign-in window loads.
#[cfg(desktop)]
const POPUP_INIT_SCRIPT: &str = r"
(function () {
    'use strict';
    Object.defineProperty(window, 'opener', {
        configurable: true,
        get: function () {
            return {
                closed: false,
                postMessage: function (data) {
                    var payload = (typeof data === 'string') ? data : JSON.stringify(data);
                    if (window.__TAURI_INTERNALS__) {
                        // Emitted as a core event rather than sent to an app
                        // command. Tauri 2.11 began applying the ACL to app
                        // commands called from a remote origin, and this page
                        // is remote — it is thirdweb's callback. Core event
                        // emit is a plugin command, which the oauth-popup
                        // capability already grants.
                        window.__TAURI_INTERNALS__
                            .invoke('plugin:event|emit', {
                                event: 'oauth_postmessage',
                                payload: payload,
                            })
                            .catch(function (e) { console.error('[wanderer popup]', e); });
                    }
                },
            };
        },
    });
})();
";

const POPUP: &str = "oauth_popup";

/// Where the system browser returns a phone's sign-in result.
#[cfg(mobile)]
const REDIRECT: &str = "singula://oauth";

/// Opens the sign-in page at `url` in the system browser, to come back to
/// [`REDIRECT`].
#[cfg(mobile)]
#[tauri::command]
pub(crate) async fn open_popup_window(app: AppHandle, url: String) -> Result<(), String> {
    let parsed = url
        .parse::<tauri::Url>()
        .map_err(|e| format!("invalid popup url: {e}"))?;
    if parsed.scheme() != "https" {
        return Err("popup url must use https".into());
    }
    #[allow(
        deprecated,
        reason = "the opener plugin would be one more for one call"
    )]
    tauri_plugin_shell::ShellExt::shell(&app)
        .open(with_redirect(&parsed).as_str(), None)
        .map_err(|e| format!("cannot open the browser: {e}"))
}

/// thirdweb's sign-in URL (`…/login/<provider>`) told to redirect to
/// [`REDIRECT`] instead of posting to an opener.
#[cfg(mobile)]
fn with_redirect(url: &tauri::Url) -> tauri::Url {
    if url.query_pairs().any(|(k, _)| k == "redirectUrl") {
        return url.clone();
    }
    let provider = url.path_segments().and_then(|segments| {
        let parts: Vec<&str> = segments.collect();
        let at = parts.iter().position(|s| *s == "login")?;
        parts.get(at + 1).copied().filter(|p| !p.is_empty())
    });
    let (Some(provider), Ok(mut redirect)) = (provider, REDIRECT.parse::<tauri::Url>()) else {
        return url.clone();
    };
    redirect
        .query_pairs_mut()
        .append_pair("walletId", "inApp")
        .append_pair("authProvider", provider);
    let mut out = url.clone();
    out.query_pairs_mut()
        .append_pair("redirectUrl", redirect.as_str());
    out
}

/// Opens the sign-in window at `url`.
#[cfg(desktop)]
#[tauri::command]
pub(crate) async fn open_popup_window(app: AppHandle, url: String) -> Result<(), String> {
    // Only a web page: never a file or a custom scheme in a window that can
    // reach the app.
    let parsed = url
        .parse::<tauri::Url>()
        .map_err(|e| format!("invalid popup url: {e}"))?;
    if parsed.scheme() != "https" {
        return Err("popup url must use https".into());
    }
    if let Some(w) = app.get_webview_window(POPUP) {
        w.close().ok();
    }
    let window = tauri::WebviewWindowBuilder::new(&app, POPUP, tauri::WebviewUrl::External(parsed))
        .title("Sign in")
        .inner_size(520.0, 680.0)
        .center()
        .initialization_script(POPUP_INIT_SCRIPT)
        .build()
        .map_err(|e| e.to_string())?;

    // thirdweb polls the stand-in popup's `closed`; it must turn true when
    // the person closes the window too, or sign-in waits forever.
    let handle = app.clone();
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            handle.emit("oauth_popup_closed", ()).ok();
        }
    });
    Ok(())
}

/// Closes the sign-in window once its result has been posted.
pub(crate) fn close_popup_on_result(app: &AppHandle) {
    let handle = app.clone();
    app.listen_any("oauth_postmessage", move |_| {
        // Closed is signalled first, so the main window flips the stand-in's
        // `closed` before thirdweb's next poll.
        handle.emit("oauth_popup_closed", ()).ok();
        if let Some(w) = handle.get_webview_window(POPUP) {
            w.close().ok();
        }
    });
}
