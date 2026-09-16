//! Writes a macOS `.iconset` directory from the bundled artwork.
//!
//! `package.py` runs this and then assembles the `.icns` itself, which keeps
//! packaging independent of `iconutil` and the services it needs — those are
//! unavailable in some build sandboxes.
//!
//! Usage: `den-icons <iconset-directory>`

use std::path::PathBuf;
use std::process::ExitCode;

use den::icon;

/// `icon_<name>.png` at the source size Apple expects for it.
const REPRESENTATIONS: [(&str, u32); 10] = [
    ("16x16", 16),
    ("16x16@2x", 32),
    ("32x32", 32),
    ("32x32@2x", 64),
    ("128x128", 128),
    ("128x128@2x", 256),
    ("256x256", 256),
    ("256x256@2x", 512),
    ("512x512", 512),
    ("512x512@2x", 1024),
];

fn main() -> ExitCode {
    let Some(directory) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: den-icons <iconset-directory>");
        return ExitCode::FAILURE;
    };

    if let Err(error) = std::fs::create_dir_all(&directory) {
        eprintln!("den-icons: cannot create {}: {error}", directory.display());
        return ExitCode::FAILURE;
    }

    for (name, size) in REPRESENTATIONS {
        let Some(png) = icon::ember_png(size) else {
            eprintln!("den-icons: no bundled artwork at {size}px");
            return ExitCode::FAILURE;
        };
        let file = directory.join(format!("icon_{name}.png"));
        if let Err(error) = std::fs::write(&file, png) {
            eprintln!("den-icons: cannot write {}: {error}", file.display());
            return ExitCode::FAILURE;
        }
    }

    println!("{}", directory.display());
    ExitCode::SUCCESS
}
