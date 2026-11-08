//! Windows notifications from other apps (Discord, ...) read through `UserNotificationListener` on a background
//! thread: new toasts in the notification center are forwarded once, as (app, title, body). Windows asks the user
//! once for access ("Let apps access your notifications"); without it nothing is read.
use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::UI::Notifications::Management::{UserNotificationListener, UserNotificationListenerAccessStatus};
use windows::UI::Notifications::{KnownNotificationBindings, NotificationKinds, UserNotification};

const POLL: Duration = Duration::from_millis(1500);

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
    /// Still asking Windows.
    Pending,
    Allowed,
    /// Turned off in Windows Settings > Privacy > Notifications.
    Denied,
    /// This Windows cannot share notifications with desktop apps.
    Unavailable,
}

pub struct AppToasts {
    rx: Receiver<AppToast>,
    access: Arc<Mutex<Access>>,
}

impl AppToasts {
    /// Starts listening; new toasts arrive through `poll`.
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel();
        let access = Arc::new(Mutex::new(Access::Pending));
        let a = access.clone();
        let spawned = std::thread::Builder::new().name("app-toasts".into()).spawn(move || {
            let result = listen(&tx, &a);
            if let Ok(mut s) = a.lock()
                && *s == Access::Pending
            {
                *s = if result.is_err() { Access::Unavailable } else { Access::Denied };
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

fn read(n: &UserNotification) -> windows::core::Result<AppToast> {
    let app = n.AppInfo()?.DisplayInfo()?.DisplayName()?.to_string();
    let binding = n.Notification()?.Visual()?.GetBinding(&KnownNotificationBindings::ToastGeneric()?)?;
    let texts = binding.GetTextElements()?;
    let lines: Vec<String> =
        (0..texts.Size()?).filter_map(|i| texts.GetAt(i).ok()?.Text().ok()).map(|t| t.to_string()).collect();
    let mut lines = lines.into_iter().filter(|l| !l.trim().is_empty());
    let title = lines.next().unwrap_or_else(|| app.clone());
    let body = lines.collect::<Vec<_>>().join(" ");
    Ok(AppToast { app, title, body })
}

/// Toasts in the notification center now, with their ids.
fn current(listener: &UserNotificationListener) -> windows::core::Result<Vec<(u32, UserNotification)>> {
    let list = listener.GetNotificationsAsync(NotificationKinds::Toast)?.get()?;
    Ok((0..list.Size()?).filter_map(|i| list.GetAt(i).ok()).filter_map(|n| Some((n.Id().ok()?, n))).collect())
}

fn listen(tx: &Sender<AppToast>, access: &Mutex<Access>) -> windows::core::Result<()> {
    let listener = UserNotificationListener::Current()?;
    let status = listener.RequestAccessAsync()?.get()?;
    let allowed = status == UserNotificationListenerAccessStatus::Allowed;
    if let Ok(mut a) = access.lock() {
        *a = if allowed { Access::Allowed } else { Access::Denied };
    }
    if !allowed {
        return Ok(());
    }
    // What is already in the notification center when we start is old news.
    let mut seen: HashSet<u32> = current(&listener)?.into_iter().map(|(id, _)| id).collect();
    loop {
        std::thread::sleep(POLL);
        let now = current(&listener)?;
        for (id, n) in &now {
            if seen.insert(*id)
                && let Ok(t) = read(n)
                && tx.send(t).is_err()
            {
                return Ok(());
            }
        }
        // Dismissed toasts drop out, so the set never grows past what the notification center holds.
        seen.retain(|id| now.iter().any(|(n, _)| n == id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run by hand: `cargo test -p dvr-sys toasts -- --ignored --nocapture`, then send yourself a Discord DM.
    #[test]
    #[ignore = "needs a desktop session and Windows notification access"]
    fn probe_notification_access() {
        let t = AppToasts::start();
        for _ in 0..40 {
            std::thread::sleep(Duration::from_millis(500));
            while let Some(toast) = t.poll() {
                eprintln!("toast: {toast:?}");
            }
            if t.access() != Access::Pending && t.access() != Access::Allowed {
                break;
            }
        }
        eprintln!("access: {:?}", t.access());
    }
}
