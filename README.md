# mewsic

A minimal MPD frontend written in Rust with egui. It runs on Linux (X11 and Wayland), macOS and Windows.

- Browse your music by folder, artist, album or genre, using the tabs above the side panel, and search within each tab. Any of them can be played or opened as a queue.
- Filter tracks: `morning`, `artist:alpha`, `album:"night songs"`, `-live`. The available fields are `title`, `artist`, `albumartist`, `album`, `genre`, `date` and `path`.
- Keep named queues as tabs. They are saved to disk and come back when you reopen them from **Saved queues**. The tab marked ▶ is the one MPD is playing. Starting another tab saves the current one's position first.
- Album art comes from MPD (embedded pictures or cover files).
- A ReplayGain mode selector switches between off, track, album and auto.
- Even out loudness with **Scan ReplayGain** from the right-click menu of tracks, or of an album, artist, folder or genre in the side panel. It writes ReplayGain tags to the files using [rsgain](https://github.com/complexlogic/rsgain), which must be on your `PATH`, and then MPD re-reads them. Album gain is measured over the tracks you scan from each album, so scan whole albums for a correct album gain. The first scan asks for MPD's music folder; you can change it later in ⚙ Settings.
- 🔀 toggles shuffle (MPD's random mode) and 🔁 cycles between no loop, loop queue and loop track (🔂).
- × at the top right quits. Closing mewsic, by that button or the window, stops playback.

## Running

```sh
cargo run --release
```

It connects to `MPD_HOST` / `MPD_PORT` and defaults to `localhost:6600`. `MPD_HOST` also accepts `password@host`, or a socket path on Unix.

`mewsic --toggle`, `mewsic --next` and `mewsic --previous` control playback without opening a window, so they also work from keybindings and scripts.

Nix users can run `devenv shell` first to get the toolchain, rsgain, and the graphics and font libraries.

### Installing on Nix

`package.nix` builds mewsic with its runtime libraries and rsgain wrapped in, and installs a desktop entry so it shows up in application launchers. The entry also adds Play / Pause, Next and Previous to the right-click menu of mewsic's icon in docks and bars:

```sh
nix profile install git+file:///path/to/mewsic
```

After changing the source, rebuild and replace the installed copy with:

```sh
nix profile upgrade mewsic
```

The flake only sees files tracked by git, so `git add` new files before upgrading.

On NixOS you can add `(pkgs.callPackage /path/to/mewsic/package.nix {})` to `environment.systemPackages` instead.

Settings are stored as `settings.json` in the platform config directory:

- Linux: `~/.config/mewsic`
- macOS: `~/Library/Application Support/mewsic`
- Windows: `%APPDATA%\mewsic\config`

Queues are stored as JSON in the platform data directory:

- Linux: `~/.local/share/mewsic/queues`
- macOS: `~/Library/Application Support/mewsic/queues`
- Windows: `%APPDATA%\mewsic\data\queues`

## Keys

| Key | Action |
| --- | --- |
| Space | Play / pause |
| Ctrl/Cmd+F | Focus the filter |
| Esc | Clear the filter |
| Ctrl/Cmd+A | Select every track shown |
| Delete | Remove the selected tracks from a queue |
| Double-click | Play a track |
| Right-click | More options |
| Drag | Reorder a queue |

## Tests

```sh
cargo test
MPD_PORT=6600 cargo test -- --ignored   # replaces the queue of that MPD
```
