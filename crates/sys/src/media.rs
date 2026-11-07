//! Now-playing info and transport controls through Windows' media session (SMTC), on a background thread
//! so the blocking WinRT calls never touch the render loop.
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    pub playing: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaCommand {
    PlayPause,
    Next,
    Previous,
}

pub struct Media {
    info: Arc<Mutex<Option<MediaInfo>>>,
    commands: Sender<MediaCommand>,
}

const POLL: Duration = Duration::from_secs(1);

fn read(manager: &Manager) -> Option<MediaInfo> {
    let session = manager.GetCurrentSession().ok()?;
    let props = session.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
    let title = props.Title().ok()?.to_string_lossy();
    if title.is_empty() {
        return None;
    }
    let playing = session.GetPlaybackInfo().and_then(|p| p.PlaybackStatus()).is_ok_and(|s| s == Status::Playing);
    Some(MediaInfo { title, artist: props.Artist().map(|a| a.to_string_lossy()).unwrap_or_default(), playing })
}

fn control(manager: &Manager, cmd: MediaCommand) {
    let Ok(session) = manager.GetCurrentSession() else { return };
    let op = match cmd {
        MediaCommand::PlayPause => session.TryTogglePlayPauseAsync(),
        MediaCommand::Next => session.TrySkipNextAsync(),
        MediaCommand::Previous => session.TrySkipPreviousAsync(),
    };
    // A player that refuses a command is not an error worth surfacing.
    let _ = op.and_then(|o| o.get());
}

impl Media {
    /// Starts the media thread; without a media session service the info simply stays empty.
    pub fn start() -> Self {
        let info = Arc::new(Mutex::new(None));
        let (commands, rx) = mpsc::channel();
        let shared = info.clone();
        std::thread::spawn(move || {
            let Ok(manager) = Manager::RequestAsync().and_then(|op| op.get()) else { return };
            loop {
                let latest = read(&manager);
                if let Ok(mut i) = shared.lock() {
                    *i = latest;
                }
                match rx.recv_timeout(POLL) {
                    Ok(cmd) => control(&manager, cmd),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        });
        Self { info, commands }
    }

    pub fn info(&self) -> Option<MediaInfo> {
        self.info.lock().ok().and_then(|i| i.clone())
    }

    pub fn send(&self, cmd: MediaCommand) {
        // The thread may have ended (no media service); then there is nothing to control.
        let _ = self.commands.send(cmd);
    }
}
