// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! Notifications from other apps (Discord, ...): Linux apps send them to the desktop's notification service over
//! the session bus, so WristXR watches those calls as a bus monitor (no permission prompt; the desktop still shows
//! them as usual).
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use zbus::MatchRule;
use zbus::blocking::{Connection, MessageIterator};
use zbus::message::Type;
use zbus::zvariant::OwnedValue;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppToast {
    /// App display name, e.g. "Discord".
    pub app: String,
    /// First line (for Discord: the sender, and channel / server).
    pub title: String,
    /// Remaining lines.
    pub body: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// Still connecting.
    Pending,
    Allowed,
    /// The bus refused to let us watch.
    Denied,
    /// No session bus.
    Unavailable,
}

pub struct AppToasts {
    rx: Receiver<AppToast>,
    access: Arc<Mutex<Access>>,
}

type Notify<'a> = (String, u32, String, String, String, Vec<String>, HashMap<String, OwnedValue>, i32);

fn listen(tx: &Sender<AppToast>, access: &Mutex<Access>) -> Result<(), Access> {
    let set = |a: Access| {
        if let Ok(mut s) = access.lock() {
            *s = a;
        }
    };
    let conn = Connection::session().map_err(|_| Access::Unavailable)?;
    let rule = MatchRule::try_from("type='method_call',interface='org.freedesktop.Notifications',member='Notify'")
        .map_err(|_| Access::Unavailable)?;
    zbus::blocking::fdo::MonitoringProxy::new(&conn)
        .map_err(|_| Access::Unavailable)?
        .become_monitor(&[rule], 0)
        .map_err(|_| Access::Denied)?;
    set(Access::Allowed);
    for msg in MessageIterator::from(&conn) {
        let Ok(msg) = msg else { continue };
        let header = msg.header();
        if msg.message_type() != Type::MethodCall || header.member().is_none_or(|m| m.as_str() != "Notify") {
            continue;
        }
        let Ok((app, _, _, summary, body, ..)) = msg.body().deserialize::<Notify<'_>>() else { continue };
        let toast = AppToast {
            app: if app.is_empty() { "App".into() } else { app },
            title: summary,
            body: strip_markup(&body),
        };
        if tx.send(toast).is_err() {
            return Ok(());
        }
    }
    Ok(())
}

/// Notification bodies may carry a little HTML-like markup; the watch shows plain text.
fn strip_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut tag = false;
    for c in s.chars() {
        match c {
            '<' => tag = true,
            '>' if tag => tag = false,
            _ if !tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'")
}

impl AppToasts {
    /// Starts listening; new toasts arrive through `poll`.
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel();
        let access = Arc::new(Mutex::new(Access::Pending));
        let a = access.clone();
        let spawned = std::thread::Builder::new().name("app-toasts".into()).spawn(move || {
            if let Err(state) = listen(&tx, &a)
                && let Ok(mut s) = a.lock()
            {
                *s = state;
            }
        });
        if spawned.is_err()
            && let Ok(mut s) = access.lock()
        {
            *s = Access::Unavailable;
        }
        Self { rx, access }
    }

    pub fn access(&self) -> Access {
        self.access.lock().map_or(Access::Unavailable, |a| *a)
    }

    /// Next new toast, if any (non-blocking).
    pub fn poll(&self) -> Option<AppToast> {
        self.rx.try_recv().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::strip_markup;

    #[test]
    fn markup_goes() {
        assert_eq!(strip_markup("<b>Alex</b> &amp; Mira"), "Alex & Mira");
    }
}
