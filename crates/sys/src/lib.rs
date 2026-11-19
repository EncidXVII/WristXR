//! The operating system around the VR session: load and time for the watch, media and app notifications, one
//! instance, dialogs. Windows (`win`) and Linux (`linux`) offer the same API.
#![allow(unsafe_code)]

#[cfg(windows)]
mod win;
#[cfg(windows)]
pub use win::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;
