//! Den — a desktop client for the den.nvim Markdown vault.

use std::process::ExitCode;

use den::Den;
use den::cli::{self, Outcome};
use den::{icon, widget};

fn main() -> ExitCode {
    let options = match cli::parse(std::env::args().skip(1)) {
        Outcome::Run(options) => options,
        Outcome::ShowUsage => {
            print!("{}", cli::USAGE);
            return ExitCode::SUCCESS;
        }
        Outcome::ListFonts => {
            for family in den::fonts::installed() {
                println!("{family}");
            }
            return ExitCode::SUCCESS;
        }
        Outcome::Invalid(error) => {
            eprintln!("den: {error}\n");
            eprint!("{}", cli::USAGE);
            return ExitCode::FAILURE;
        }
    };

    let window = iced::window::Settings {
        size: iced::Size::new(1440.0, 920.0),
        min_size: Some(iced::Size::new(960.0, 600.0)),
        icon: icon::window_icon(options.variant),
        // Native decorations, deliberately. Every hand-drawn title bar here
        // was a band duplicating what the platform already draws well — and
        // the window title is a better home for the vault name than a row of
        // our own. The app draws the app; the OS draws the window.
        ..iced::window::Settings::default()
    };

    let result = iced::application(
        move || Den::new(options.clone()),
        Den::update,
        widget::chrome::view,
    )
    .title(Den::title)
    .theme(Den::theme)
    .subscription(Den::subscription)
    // The bundled face, so the design has the font it was drawn against on a
    // machine that has never heard of it. Without this `Font::with_name` would
    // not resolve and iced would fall silently back to a proportional face —
    // the same trap `fonts.rs` exists to avoid.
    .font(den::fonts::BUNDLED_REGULAR)
    .font(den::fonts::BUNDLED_MEDIUM)
    // Without this the `--scale` flag and ⌘+/⌘- are silently inert: the state
    // changes and nothing redraws differently.
    .scale_factor(Den::scale_factor)
    .window(window)
    .antialiasing(true)
    .run();

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("den: {error}");
            ExitCode::FAILURE
        }
    }
}
