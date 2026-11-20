// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! Puts the icons into the Windows executable. Group 1 is the app icon (Explorer, taskbar, Alt+Tab); 2 and 3 are
//! the tray icon for dark and light taskbars. They come from packaging/brand (drawn by marks.py) and go in as a
//! compiled resource file, which the MSVC linker takes as an input, so no resource compiler is needed.

use std::path::{Path, PathBuf};

const ICONS: [(u16, &str); 3] = [(1, "wristxr.ico"), (2, "wristxr-tray.ico"), (3, "wristxr-tray-ink.ico")];
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/brand");
    for (_, file) in ICONS {
        println!("cargo:rerun-if-changed={}", dir.join(file).display());
    }
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    if var("CARGO_CFG_TARGET_OS") != "windows" || var("CARGO_CFG_TARGET_ENV") != "msvc" {
        return;
    }
    let mut res = Vec::new();
    entry(&mut res, 0, 0, &[]); // every .res file opens with an empty entry
    let mut image_id = 1;
    for (group, file) in ICONS {
        let ico = std::fs::read(dir.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        let count = usize::from(u16::from_le_bytes([ico[4], ico[5]]));
        // The group lists its images like the .ico does, with a resource id where the .ico has a file offset.
        let mut group_dir = vec![0, 0, 1, 0];
        group_dir.extend_from_slice(&ico[4..6]);
        for i in 0..count {
            let e = &ico[6 + 16 * i..6 + 16 * (i + 1)];
            let len = u32::from_le_bytes([e[8], e[9], e[10], e[11]]) as usize;
            let at = u32::from_le_bytes([e[12], e[13], e[14], e[15]]) as usize;
            entry(&mut res, RT_ICON, image_id, &ico[at..at + len]);
            group_dir.extend_from_slice(&e[..12]);
            group_dir.extend_from_slice(&image_id.to_le_bytes());
            image_id += 1;
        }
        entry(&mut res, RT_GROUP_ICON, group, &group_dir);
    }
    let out = PathBuf::from(var("OUT_DIR")).join("icons.res");
    std::fs::write(&out, res).unwrap_or_else(|e| panic!("{}: {e}", out.display()));
    println!("cargo:rustc-link-arg-bins={}", out.display());
}

/// One resource in the 32-bit .res format: a 32-byte header with numeric type and id, then the data, padded to 4.
fn entry(res: &mut Vec<u8>, kind: u16, id: u16, data: &[u8]) {
    let (flags, lang): (u16, u16) = match kind {
        0 => (0, 0),
        RT_ICON => (0x1010, 0x0409),
        _ => (0x1030, 0x0409),
    };
    res.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_le_bytes());
    res.extend_from_slice(&32u32.to_le_bytes());
    res.extend_from_slice(&[0xFF, 0xFF]);
    res.extend_from_slice(&kind.to_le_bytes());
    res.extend_from_slice(&[0xFF, 0xFF]);
    res.extend_from_slice(&id.to_le_bytes());
    res.extend_from_slice(&0u32.to_le_bytes()); // data version
    res.extend_from_slice(&flags.to_le_bytes());
    res.extend_from_slice(&lang.to_le_bytes());
    res.extend_from_slice(&0u32.to_le_bytes()); // version
    res.extend_from_slice(&0u32.to_le_bytes()); // characteristics
    res.extend_from_slice(data);
    while !res.len().is_multiple_of(4) {
        res.push(0);
    }
}
