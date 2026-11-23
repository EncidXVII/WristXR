// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! Now-playing info, transport controls and the player's volume, on a background thread so the blocking WinRT and
//! COM calls never touch the render loop. Media goes through Windows' media session (SMTC). The volume is the
//! player's own (its Volume mixer entry), on whichever output device it plays through: a player sent to a device
//! of its own (Spotify to an audio interface while Windows' default is the headset) is not turned down by the
//! default device's volume, and devices that take volume themselves (Virtual Desktop's) may not apply it at all. A
//! player with no audio of its own (or an id that names no program) falls back to the default device's volume.
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapBounds, BitmapDecoder, BitmapInterpolationMode, BitmapPixelFormat, BitmapTransform,
    ColorManagementMode, ExifOrientationMode,
};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionMediaProperties as Properties,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, IAudioSessionControl2, IAudioSessionManager2, IMMDevice, IMMDeviceEnumerator,
    ISimpleAudioVolume, MMDeviceEnumerator, eConsole, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::core::{Interface, PWSTR};

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
    /// The player takes a shuffle change (a browser tab usually does not).
    pub can_shuffle: bool,
    pub at: Instant,
    /// The song's cover, `COVER_PX` square, RGBA (None when the player gives none).
    pub cover: Option<Arc<[u8]>>,
}

/// Side of a song's cover in pixels: sharp on the watch's largest tile (72 px).
pub const COVER_PX: u32 = 128;

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
    /// The player's volume, 0..1 (the output device's when the player has no audio of its own there).
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

/// The last song and its cover: read once per song, and again while missing (a player may set it a moment after
/// the title).
#[derive(Default)]
struct Covers {
    song: (String, String),
    cover: Option<Arc<[u8]>>,
}

/// A cover from the media session: the middle square of its thumbnail, scaled to `COVER_PX`.
fn cover(props: &Properties) -> Option<Arc<[u8]>> {
    let stream = props.Thumbnail().ok()?.OpenReadAsync().ok()?.join().ok()?;
    let decoder = BitmapDecoder::CreateAsync(&stream).ok()?.join().ok()?;
    let (w, h) = (decoder.OrientedPixelWidth().ok()?, decoder.OrientedPixelHeight().ok()?);
    let side = w.min(h);
    if side == 0 {
        return None;
    }
    let scaled = |n: u32| ((u64::from(n) * u64::from(COVER_PX)).div_ceil(u64::from(side)) as u32).max(COVER_PX);
    let (sw, sh) = (scaled(w), scaled(h));
    let t = BitmapTransform::new().ok()?;
    t.SetScaledWidth(sw).ok()?;
    t.SetScaledHeight(sh).ok()?;
    t.SetInterpolationMode(BitmapInterpolationMode::Fant).ok()?;
    t.SetBounds(BitmapBounds { X: (sw - COVER_PX) / 2, Y: (sh - COVER_PX) / 2, Width: COVER_PX, Height: COVER_PX })
        .ok()?;
    let pixels = decoder
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Rgba8,
            BitmapAlphaMode::Straight,
            &t,
            ExifOrientationMode::RespectExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )
        .ok()?
        .join()
        .ok()?
        .DetachPixelData()
        .ok()?;
    (pixels.len() == (COVER_PX * COVER_PX * 4) as usize).then(|| Arc::from(&pixels[..]))
}

fn read(manager: &Manager, covers: &mut Covers) -> Option<MediaInfo> {
    let session = manager.GetCurrentSession().ok()?;
    let props = session.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
    let title = props.Title().ok()?.to_string_lossy();
    if title.is_empty() {
        return None;
    }
    let artist = props.Artist().map(|a| a.to_string_lossy()).unwrap_or_default();
    let song = (title.clone(), artist.clone());
    if covers.song != song || covers.cover.is_none() {
        covers.cover = cover(&props);
        covers.song = song;
    }
    let info = session.GetPlaybackInfo().ok();
    let playing = info.as_ref().and_then(|p| p.PlaybackStatus().ok()) == Some(Status::Playing);
    let can_shuffle =
        info.as_ref().and_then(|p| p.Controls().ok()).and_then(|c| c.IsShuffleEnabled().ok()).unwrap_or(false);
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
        artist,
        app,
        playing,
        position,
        duration,
        shuffle,
        can_shuffle,
        at: Instant::now(),
        cover: covers.cover.clone(),
    })
}

/// What the volume slider and mute act on.
enum Volume {
    /// The player's audio sessions (a browser can have several, a player one per device); all are set, the first
    /// is read.
    App(Vec<ISimpleAudioVolume>),
    /// The output device, for a player with no session of its own.
    Device(IAudioEndpointVolume),
}

impl Volume {
    /// For the player with media session id `player`, on every output device it plays through.
    fn of(player: &str) -> Option<Self> {
        // SAFETY: COM is initialised on this thread; the interfaces are released when dropped.
        unsafe {
            let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let mut apps = Vec::new();
            if let Ok(all) = devices.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) {
                for i in 0..all.GetCount().unwrap_or(0) {
                    if let Ok(d) = all.Item(i) {
                        apps.extend(app_sessions(&d, player));
                    }
                }
            }
            let device = devices.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
            if apps.is_empty() {
                device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok().map(Self::Device)
            } else {
                Some(Self::App(apps))
            }
        }
    }

    fn read(&self) -> Option<(f32, bool)> {
        // SAFETY: plain queries on live interfaces.
        unsafe {
            match self {
                Self::App(v) => v.first().and_then(|v| Some((v.GetMasterVolume().ok()?, v.GetMute().ok()?.as_bool()))),
                Self::Device(v) => Some((v.GetMasterVolumeLevelScalar().ok()?, v.GetMute().ok()?.as_bool())),
            }
        }
    }

    // A session or device that refuses a change is not an error worth surfacing.
    fn set(&self, level: f32) {
        let level = level.clamp(0.0, 1.0);
        // SAFETY: plain setters on live interfaces; a null event context is allowed.
        unsafe {
            match self {
                Self::App(v) => v.iter().for_each(|v| drop(v.SetMasterVolume(level, std::ptr::null()))),
                Self::Device(v) => drop(v.SetMasterVolumeLevelScalar(level, std::ptr::null())),
            }
        }
    }

    fn toggle_mute(&self) {
        let Some((_, muted)) = self.read() else { return };
        // SAFETY: as above.
        unsafe {
            match self {
                Self::App(v) => v.iter().for_each(|v| drop(v.SetMute(!muted, std::ptr::null()))),
                Self::Device(v) => drop(v.SetMute(!muted, std::ptr::null())),
            }
        }
    }
}

/// The volume controls of `player`'s audio sessions on `device`: those of a program `plays_as` it.
fn app_sessions(device: &IMMDevice, player: &str) -> Vec<ISimpleAudioVolume> {
    let mut out = Vec::new();
    if player.is_empty() {
        return out;
    }
    // SAFETY: COM calls on live interfaces; the process handle is closed on every path.
    unsafe {
        let Ok(list) =
            device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None).and_then(|m| m.GetSessionEnumerator())
        else {
            return out;
        };
        for i in 0..list.GetCount().unwrap_or(0) {
            let Ok(session) = list.GetSession(i) else { continue };
            let Ok(pid) = session.cast::<IAudioSessionControl2>().and_then(|s| s.GetProcessId()) else { continue };
            // 0 is Windows' own sounds.
            if pid != 0
                && exe_of(pid).is_some_and(|exe| plays_as(&exe, player))
                && let Ok(v) = session.cast::<ISimpleAudioVolume>()
            {
                out.push(v);
            }
        }
    }
    out
}

/// The full path of process `pid`'s program.
fn exe_of(pid: u32) -> Option<String> {
    // SAFETY: the buffer outlives the call and its length is passed; the handle is closed before returning.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// True when the program at `exe` is the player whose media session id is `player`: "chrome" for chrome.exe,
/// "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify" for Spotify.exe. An opaque id (Firefox's) matches nothing.
fn plays_as(exe: &str, player: &str) -> bool {
    let file = exe.rsplit(['\\', '/']).next().unwrap_or(exe).to_lowercase();
    let stem = file.strip_suffix(".exe").unwrap_or(&file);
    let name = app_name(player).to_lowercase();
    !stem.is_empty() && (stem == name || (stem.len() >= 4 && player.to_lowercase().contains(stem)))
}

fn control(session: Option<&Session>, volume: Option<&Volume>, cmd: MediaCommand) {
    // A player that refuses a command is not an error worth surfacing.
    match cmd {
        MediaCommand::Volume(f) => {
            if let Some(v) = volume {
                v.set(f);
            }
        }
        MediaCommand::ToggleMute => {
            if let Some(v) = volume {
                v.toggle_mute();
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
            let _ = op.and_then(|o| o.join());
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
            // SAFETY: initialises COM for this thread only (needed for the volume).
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            let manager = Manager::RequestAsync().and_then(|op| op.join()).ok();
            let mut covers = Covers::default();
            loop {
                let media = manager.as_ref().and_then(|m| read(m, &mut covers));
                let session = manager.as_ref().and_then(|m| m.GetCurrentSession().ok());
                let player = session.as_ref().and_then(|s| s.SourceAppUserModelId().ok()).unwrap_or_default();
                let volume = Volume::of(&player.to_string_lossy());
                if let Ok(mut s) = out.lock() {
                    s.media = media;
                    s.volume = volume.as_ref().and_then(Volume::read);
                }
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

    /// The player's volume (0..1) and mute, or the output device's (see `MediaCommand::Volume`).
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
    fn players_are_found_by_their_program() {
        assert!(plays_as(r"C:\Users\a\AppData\Roaming\Spotify\Spotify.exe", "Spotify.exe"));
        assert!(plays_as(
            r"C:\Program Files\WindowsApps\Spotify\Spotify.exe",
            "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"
        ));
        assert!(plays_as(r"C:\Program Files\Google\Chrome\Application\chrome.exe", "chrome"));
        assert!(plays_as(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe", "MSEdge"));
        assert!(plays_as(r"C:\Program Files\VideoLAN\VLC\vlc.exe", "vlc.exe"));
        // Not another program, and never on an opaque id.
        assert!(!plays_as(r"C:\Program Files\Google\Chrome\Application\chrome.exe", "Spotify.exe"));
        assert!(!plays_as(r"C:\Program Files\Mozilla Firefox\firefox.exe", "308046B0AF4A39CB"));
        assert!(!plays_as(r"C:\Windows\a.exe", "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"));
    }

    #[test]
    fn player_names_are_readable_or_empty() {
        assert_eq!(app_name("Spotify.exe"), "Spotify");
        assert_eq!(app_name("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"), "Spotify");
        assert_eq!(app_name("chrome"), "Chrome");
        assert_eq!(app_name("308046B0AF4A39CB"), "");
        assert_eq!(app_name(""), "");
    }

    /// Against the real player (changes it, then puts it back): `cargo test -p wxr-sys -- --ignored live_shuffle`.
    #[test]
    #[ignore]
    fn live_shuffle() {
        let m = Media::start();
        let wait = |f: &dyn Fn(&MediaInfo) -> bool| {
            for _ in 0..50 {
                if m.info().is_some_and(|i| f(&i)) {
                    return true;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            false
        };
        assert!(wait(&|_| true), "no media");
        let before = m.info().unwrap().shuffle;
        eprintln!("player {} shuffle {before}", m.info().unwrap().app);
        m.send(MediaCommand::ToggleShuffle);
        let changed = wait(&|i| i.shuffle != before);
        eprintln!("after toggle: changed={changed}");
        m.send(MediaCommand::ToggleShuffle);
        let back = wait(&|i| i.shuffle == before);
        eprintln!("after second toggle: back={back}");
        assert!(changed && back);
    }
}
