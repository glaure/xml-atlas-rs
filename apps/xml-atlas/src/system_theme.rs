//! The desktop's light or dark preference, on Linux (`docs/spec/window.md`, WIN-2).
//!
//! winit does not report the system theme on Linux, so egui would always fall back to dark.
//! The freedesktop settings portal knows it: `org.freedesktop.appearance` / `color-scheme`,
//! answered by GNOME, KDE and others. A background thread reads it once and then listens for
//! changes; without a session bus or a portal it reports nothing and the app falls back to
//! egui's default. On other systems winit reports the theme itself, and this module does
//! nothing.

use std::sync::mpsc::Receiver;

use egui::Theme;

/// Starts watching the desktop's preference. The receiver gets the theme at once (if the
/// portal answers) and again whenever it changes; `repaint` wakes the window for it.
pub fn watch(repaint: egui::Context) -> Option<Receiver<Theme>> {
    imp::watch(repaint)
}

/// The portal's `color-scheme` value: 1 prefers dark, 2 prefers light, 0 has no preference.
fn theme_of(color_scheme: u32) -> Option<Theme> {
    match color_scheme {
        1 => Some(Theme::Dark),
        2 => Some(Theme::Light),
        _ => None,
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::sync::mpsc::{Receiver, Sender, channel};

    use egui::Theme;
    use zbus::blocking::{Connection, Proxy};
    use zbus::zvariant::{OwnedValue, Value};

    const DESTINATION: &str = "org.freedesktop.portal.Desktop";
    const PATH: &str = "/org/freedesktop/portal/desktop";
    const INTERFACE: &str = "org.freedesktop.portal.Settings";
    const NAMESPACE: &str = "org.freedesktop.appearance";
    const KEY: &str = "color-scheme";

    pub fn watch(repaint: egui::Context) -> Option<Receiver<Theme>> {
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new()
            .name("system-theme".into())
            .spawn(move || {
                if let Err(e) = run(&tx, &repaint) {
                    log::info!("system theme not available: {e}");
                }
            });
        spawned.ok().map(|_| rx)
    }

    fn run(tx: &Sender<Theme>, repaint: &egui::Context) -> zbus::Result<()> {
        let connection = Connection::session()?;
        let proxy = Proxy::new(&connection, DESTINATION, PATH, INTERFACE)?;
        // Subscribe before reading, so a change in between is not missed.
        let changes = proxy.receive_signal("SettingChanged")?;
        if let Some(theme) = read(&proxy).and_then(super::theme_of) {
            send(tx, repaint, theme);
        }
        for message in changes {
            let Ok((namespace, key, value)) =
                message.body().deserialize::<(String, String, OwnedValue)>()
            else {
                continue;
            };
            if namespace == NAMESPACE
                && key == KEY
                && let Some(theme) = scheme(&value).and_then(super::theme_of)
                && !send(tx, repaint, theme)
            {
                break;
            }
        }
        Ok(())
    }

    /// The current `color-scheme`. `ReadOne` is the current portal method; older portals only
    /// have `Read`, which wraps the value in one more variant.
    fn read(proxy: &Proxy<'_>) -> Option<u32> {
        if let Ok(v) = proxy.call::<_, _, OwnedValue>("ReadOne", &(NAMESPACE, KEY)) {
            return scheme(&v);
        }
        let v = proxy
            .call::<_, _, OwnedValue>("Read", &(NAMESPACE, KEY))
            .ok()?;
        scheme(&v)
    }

    /// A `u32`, possibly inside one or two variants.
    fn scheme(value: &Value<'_>) -> Option<u32> {
        match value {
            Value::U32(n) => Some(*n),
            Value::Value(inner) => scheme(inner),
            _ => None,
        }
    }

    /// False once the window is gone.
    fn send(tx: &Sender<Theme>, repaint: &egui::Context, theme: Theme) -> bool {
        let ok = tx.send(theme).is_ok();
        repaint.request_repaint();
        ok
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use std::sync::mpsc::Receiver;

    pub fn watch(_repaint: egui::Context) -> Option<Receiver<egui::Theme>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_values() {
        assert_eq!(theme_of(1), Some(Theme::Dark));
        assert_eq!(theme_of(2), Some(Theme::Light));
        assert_eq!(theme_of(0), None);
        assert_eq!(theme_of(7), None);
    }
}
