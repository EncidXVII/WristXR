//! Now-playing info, transport controls and the Windows volume, on a background thread so the blocking WinRT and
//! COM calls never touch the render loop. Media goes through Windows' media session (SMTC), the volume through the
//! default output device's endpoint volume.
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;

#[derive(Clone, Debug, PartialEq)]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    /// Player name ("Spotify"); empty when Windows only gives an opaque id.
    pub app: String,
    pub playing: bool,
    /// Seconds into the song when `at` was taken; `duration` 0 = unknown (a stream).
    pub position: f32,
    pub duration: f32,
    pub shuffle: bool,
    pub at: Instant,
}

impl MediaInfo {
    /// Position now, counting on from `at` while playing.
    pub fn position_now(&self) -> f32 {
        let p = self.position + if self.playing { self.at.elapsed().as_secs_f32() } else { 0.0 };
        if self.duration > 0.0 { p.min(self.duration) } else { p }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MediaCommand {
    PlayPause,
    Next,
    Previous,
    ToggleShuffle,
    /// Seconds into the song.
    Seek(f32),
    /// Windows output volume, 0..1.
    Volume(f32),
    ToggleMute,
}

#[derive(Default)]
struct Shared {
    media: Option<MediaInfo>,
    volume: Option<(f32, bool)>,
}

pub struct Media {
    shared: Arc<Mutex<Shared>>,
    commands: Sender<MediaCommand>,
}

const POLL: Duration = Duration::from_secs(1);
/// WinRT TimeSpan / DateTime ticks per second.
const TICKS: f64 = 10_000_000.0;

/// "Spotify.exe" / "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify" -> "Spotify"; opaque ids -> "".
pub fn app_name(id: &str) -> String {
    let name = id.rsplit('!').next().unwrap_or(id);
    let name = name.strip_suffix(".exe").or_else(|| name.strip_suffix(".EXE")).unwrap_or(name);
    let name = name.rsplit(['\\', '/']).next().unwrap_or(name);
    let opaque = name.len() > 8 && name.chars().all(|c| c.is_ascii_hexdigit());
    if name.is_empty() || opaque {
        return String::new();
    }
    let mut c = name.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

fn now_ticks() -> i64 {
    // SAFETY: plain query.
    let ft = unsafe { GetSystemTimeAsFileTime() };
    (i64::from(ft.dwHighDateTime) << 32) | i64::from(ft.dwLowDateTime)
}

fn read(manager: &Manager) -> Option<MediaInfo> {
    let session = manager.GetCurrentSession().ok()?;
    let props = session.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
    let title = props.Title().ok()?.to_string_lossy();
    if title.is_empty() {
        return None;
    }
    let info = session.GetPlaybackInfo().ok();
    let playing = info.as_ref().and_then(|p| p.PlaybackStatus().ok()) == Some(Status::Playing);
    let shuffle = info.and_then(|p| p.IsShuffleActive().ok()).and_then(|s| s.Value().ok()).unwrap_or(false);
    let (mut position, mut duration) = (0.0, 0.0);
    if let Ok(t) = session.GetTimelineProperties() {
        let span = |r: windows::core::Result<windows::Foundation::TimeSpan>| r.map_or(0.0, |s| s.Duration as f64);
        let (start, end) = (span(t.StartTime()), span(t.EndTime()));
        let mut pos = span(t.Position()) - start;
        // Players update the timeline only on events; count on from when they did.
        if playing && let Ok(updated) = t.LastUpdatedTime() {
            pos += (now_ticks() - updated.UniversalTime).max(0) as f64;
        }
        duration = ((end - start) / TICKS) as f32;
        position = (pos / TICKS) as f32;
        if duration > 0.0 {
            position = position.clamp(0.0, duration);
        }
    }
    let app = session.SourceAppUserModelId().map(|s| app_name(&s.to_string_lossy())).unwrap_or_default();
    Some(MediaInfo {
        title,
        artist: props.Artist().map(|a| a.to_string_lossy()).unwrap_or_default(),
        app,
        playing,
        position,
        duration,
        shuffle,
        at: Instant::now(),
    })
}

fn endpoint() -> Option<IAudioEndpointVolume> {
    // SAFETY: COM is initialised on this thread; the interfaces are released when dropped.
    unsafe {
        let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let device = devices.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()
    }
}

fn read_volume(v: &IAudioEndpointVolume) -> Option<(f32, bool)> {
    // SAFETY: plain queries on a live interface.
    unsafe { Some((v.GetMasterVolumeLevelScalar().ok()?, v.GetMute().ok()?.as_bool())) }
}

fn control(session: Option<&Session>, volume: Option<&IAudioEndpointVolume>, cmd: MediaCommand) {
    // A player or device that refuses a command is not an error worth surfacing.
    match cmd {
        MediaCommand::Volume(f) => {
            if let Some(v) = volume {
                // SAFETY: plain setter on a live interface; a null event context is allowed.
                let _ = unsafe { v.SetMasterVolumeLevelScalar(f.clamp(0.0, 1.0), std::ptr::null()) };
            }
        }
        MediaCommand::ToggleMute => {
            if let Some(v) = volume
                && let Some((_, muted)) = read_volume(v)
            {
                // SAFETY: as above.
                let _ = unsafe { v.SetMute(!muted, std::ptr::null()) };
            }
        }
        _ => {
            let Some(s) = session else { return };
            let op = match cmd {
                MediaCommand::PlayPause => s.TryTogglePlayPauseAsync(),
                MediaCommand::Next => s.TrySkipNextAsync(),
                MediaCommand::Previous => s.TrySkipPreviousAsync(),
                MediaCommand::Seek(secs) => {
                    let start = s.GetTimelineProperties().and_then(|t| t.StartTime()).map_or(0, |t| t.Duration);
                    s.TryChangePlaybackPositionAsync(start + (f64::from(secs.max(0.0)) * TICKS) as i64)
                }
                MediaCommand::ToggleShuffle => {
                    let on =
                        s.GetPlaybackInfo().and_then(|p| p.IsShuffleActive()).and_then(|v| v.Value()).unwrap_or(false);
                    s.TryChangeShuffleActiveAsync(!on)
                }
                MediaCommand::Volume(_) | MediaCommand::ToggleMute => return,
            };
            let _ = op.and_then(|o| o.get());
        }
    }
}

impl Media {
    /// Starts the media thread; without a media session service or audio device the info simply stays empty.
    pub fn start() -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (commands, rx) = mpsc::channel();
        let out = shared.clone();
        std::thread::spawn(move || {
            // SAFETY: initialises COM for this thread only (needed for the endpoint volume).
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            let manager = Manager::RequestAsync().and_then(|op| op.get()).ok();
            loop {
                let media = manager.as_ref().and_then(read);
                let volume = endpoint();
                if let Ok(mut s) = out.lock() {
                    s.media = media;
                    s.volume = volume.as_ref().and_then(read_volume);
                }
                let session = manager.as_ref().and_then(|m| m.GetCurrentSession().ok());
                match rx.recv_timeout(POLL) {
                    Ok(cmd) => {
                        control(session.as_ref(), volume.as_ref(), cmd);
                        // A slider sends many values: apply only the newest of what queued up meanwhile.
                        let mut last = None;
                        while let Ok(more) = rx.try_recv() {
                            if let Some(prev) = last.replace(more)
                                && !matches!((prev, more), (MediaCommand::Volume(_), MediaCommand::Volume(_)))
                            {
                                control(session.as_ref(), volume.as_ref(), prev);
                            }
                        }
                        if let Some(cmd) = last {
                            control(session.as_ref(), volume.as_ref(), cmd);
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        });
        Self { shared, commands }
    }

    pub fn info(&self) -> Option<MediaInfo> {
        self.shared.lock().ok().and_then(|s| s.media.clone())
    }

    /// Windows output volume (0..1) and mute.
    pub fn volume(&self) -> Option<(f32, bool)> {
        self.shared.lock().ok().and_then(|s| s.volume)
    }

    pub fn send(&self, cmd: MediaCommand) {
        // Show a new volume at once; the thread confirms it on its next read.
        if let (MediaCommand::Volume(f), Ok(mut s)) = (cmd, self.shared.lock())
            && let Some(v) = s.volume.as_mut()
        {
            v.0 = f.clamp(0.0, 1.0);
        }
        // The thread may have ended; then there is nothing to control.
        let _ = self.commands.send(cmd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_names_are_readable_or_empty() {
        assert_eq!(app_name("Spotify.exe"), "Spotify");
        assert_eq!(app_name("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"), "Spotify");
        assert_eq!(app_name("chrome"), "Chrome");
        assert_eq!(app_name("308046B0AF4A39CB"), "");
        assert_eq!(app_name(""), "");
    }
}
