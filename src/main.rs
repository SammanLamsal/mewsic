mod app;
mod art;
mod filter;
mod library;
mod mpd;
mod queues;
mod replaygain;
mod settings;
mod ui;

use eframe::egui;

const USAGE: &str = "usage: mewsic [--toggle | --next | --previous]";

fn main() -> eframe::Result {
    let config = mpd::ConnectionConfig::from_env();
    if let Some(argument) = std::env::args().nth(1) {
        run_remote_command(&config, &argument);
    }

    let Some(dirs) = directories::ProjectDirs::from("", "", "mewsic") else {
        eprintln!("mewsic: cannot determine a home directory for saving queues");
        std::process::exit(1);
    };
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../icon.png"))
        .expect("bundled icon is a valid PNG");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("mewsic")
            .with_app_id("mewsic")
            .with_icon(icon)
            .with_inner_size([1280.0, 780.0])
            .with_min_inner_size([720.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "mewsic",
        options,
        Box::new(move |creation| {
            let app = app::App::new(
                &creation.egui_ctx,
                config,
                dirs.config_dir(),
                dirs.data_dir(),
                dirs.cache_dir(),
            );
            Ok(Box::new(app))
        }),
    )
}

/// Sends one command to MPD and exits, without opening a window.
fn run_remote_command(config: &mpd::ConnectionConfig, argument: &str) -> ! {
    let Some(command) = parse_remote_command(argument) else {
        eprintln!("{USAGE}");
        std::process::exit(2);
    };

    match mpd::send_remote_command(config, command) {
        Ok(()) => std::process::exit(0),
        Err(error) => {
            eprintln!("mewsic: {error:#}");
            std::process::exit(1);
        }
    }
}

fn parse_remote_command(argument: &str) -> Option<mpd::RemoteCommand> {
    match argument {
        "--toggle" => Some(mpd::RemoteCommand::TogglePlayback),
        "--next" => Some(mpd::RemoteCommand::Next),
        "--previous" => Some(mpd::RemoteCommand::Previous),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_remote_commands() {
        assert_eq!(
            parse_remote_command("--toggle"),
            Some(mpd::RemoteCommand::TogglePlayback)
        );
        assert_eq!(
            parse_remote_command("--next"),
            Some(mpd::RemoteCommand::Next)
        );
        assert_eq!(
            parse_remote_command("--previous"),
            Some(mpd::RemoteCommand::Previous)
        );
        assert_eq!(parse_remote_command("--bogus"), None);
        assert_eq!(parse_remote_command("toggle"), None);
    }
}
