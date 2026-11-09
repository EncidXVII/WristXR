//! Line icons as SVG path data on a 24x24 grid (drawn with a 2.2 px round stroke, or filled where noted).

pub const SCREEN: &str = "M5 4h14a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z M8 20h8 M12 16v4";
pub const WINDOW: &str = "M5 4h14a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z M3 9h18";
pub const KEYS: &str = "M4 6h16a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2z M6 10h1 M10 10h1 \
                        M14 10h1 M18 10h1 M7 14h10";
pub const WATCH: &str =
    "M10 6h4a3 3 0 0 1 3 3v6a3 3 0 0 1-3 3h-4a3 3 0 0 1-3-3V9a3 3 0 0 1 3-3z M9 6V3h6v3 M9 18v3h6v-3";
pub const BELL: &str = "M6 16V11a6 6 0 0 1 12 0v5l2 2H4z M10 20a2 2 0 0 0 4 0";
pub const SLIDERS: &str = "M4 7h10 M18 7h2 M4 17h4 M12 17h8 M14 7a2 2 0 1 0 4 0a2 2 0 1 0-4 0 \
                           M8 17a2 2 0 1 0 4 0a2 2 0 1 0-4 0";
pub const CLOSE: &str = "M6 6l12 12 M18 6L6 18";
pub const BACK: &str = "M15 5l-7 7 7 7";
pub const LOCK: &str = "M8 11V8a4 4 0 0 1 8 0v3 M6 11h12v9H6z";
pub const CURVE: &str = "M3 6c6 4 12 4 18 0v12c-6-4-12-4-18 0z";
pub const OPACITY: &str = "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18z M12 3v18 M12 8h6 M12 12h8 M12 16h6";
pub const RESET: &str = "M4 12a8 8 0 1 0 2.5-5.8 M4 4v5h5";
pub const WARNING: &str = "M12 3l10 18H2z M12 10v5 M12 18v1";
pub const SHUFFLE: &str = "M3 7h4l10 10h4 M3 17h4l3-3 M14 10l3-3h4 M18 4l3 3-3 3 M18 14l3 3-3 3";
pub const SHARE: &str = "M4 12l16-8-6 16-3-7z M11 13l9-9";
pub const SPEAKER: &str = "M4 9h4l5-4v14l-5-4H4z M16 9a4 4 0 0 1 0 6 M19 6a8 8 0 0 1 0 12";
pub const MUTED: &str = "M4 9h4l5-4v14l-5-4H4z M16 9l6 6 M22 9l-6 6";
pub const NOTE: &str = "M9 18V5l11-2v13 M9 18a3 3 0 1 1-6 0a3 3 0 1 1 6 0z M20 16a3 3 0 1 1-6 0a3 3 0 1 1 6 0z";
/// Filled.
pub const PLAY: &str = "M8 5v14l11-7z";
/// Filled.
pub const PAUSE: &str = "M7 5h3v14H7z M14 5h3v14h-3z";
/// Filled.
pub const PREVIOUS: &str = "M6 5h2v14H6z M19 5v14l-10-7z";
/// Filled.
pub const NEXT: &str = "M16 5h2v14h-2z M5 5v14l10-7z";
