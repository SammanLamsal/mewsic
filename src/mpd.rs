use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, bail};
use eframe::egui;
use mpd_client::Client;
use mpd_client::client::{ConnectionEvent, ConnectionEvents, Subsystem};
use mpd_client::commands::{
    self as mpd_commands, Command as _, SeekMode, SingleMode, SongPosition,
};
use mpd_client::protocol::command::CommandList as RawCommandList;
use mpd_client::tag::Tag;
use tokio::sync::mpsc as tokio_mpsc;

use crate::art;
use crate::library::Song;

pub use mpd_client::commands::ReplayGainMode;
pub use mpd_client::responses::PlayState;

pub const REPLAY_GAIN_MODES: [ReplayGainMode; 4] = [
    ReplayGainMode::Off,
    ReplayGainMode::Track,
    ReplayGainMode::Album,
    ReplayGainMode::Auto,
];

pub fn replay_gain_label(mode: ReplayGainMode) -> &'static str {
    match mode {
        ReplayGainMode::Off => "off",
        ReplayGainMode::Track => "track",
        ReplayGainMode::Album => "album",
        ReplayGainMode::Auto => "auto",
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum LoopMode {
    #[default]
    Off,
    Queue,
    Track,
}

impl LoopMode {
    pub fn next(self) -> LoopMode {
        match self {
            LoopMode::Off => LoopMode::Queue,
            LoopMode::Queue => LoopMode::Track,
            LoopMode::Track => LoopMode::Off,
        }
    }

    fn from_mpd(repeat: bool, single: SingleMode) -> LoopMode {
        match (repeat, single) {
            (false, _) => LoopMode::Off,
            (true, SingleMode::Enabled) => LoopMode::Track,
            (true, _) => LoopMode::Queue,
        }
    }
}

const RECONNECT_DELAY: Duration = Duration::from_secs(3);
const STATUS_POLL_INTERVAL: Duration = Duration::from_millis(500);
const BINARY_LIMIT_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct ConnectionConfig {
    pub host: String,
    pub port: u16,
    pub password: Option<String>,
}

impl ConnectionConfig {
    /// Reads `MPD_HOST` (optionally `password@host`, or a socket path on Unix) and `MPD_PORT`.
    pub fn from_env() -> ConnectionConfig {
        let mut host = std::env::var("MPD_HOST").unwrap_or_else(|_| "localhost".to_string());
        let mut password = None;
        if let Some((secret, rest)) = host.split_once('@') {
            password = Some(secret.to_string());
            host = rest.to_string();
        }

        let port = std::env::var("MPD_PORT")
            .ok()
            .and_then(|port| port.parse().ok())
            .unwrap_or(6600);

        ConnectionConfig {
            host,
            port,
            password,
        }
    }

    pub fn describe(&self) -> String {
        if self.is_socket_path() {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    fn is_socket_path(&self) -> bool {
        cfg!(unix) && self.host.starts_with('/')
    }
}

#[derive(Debug)]
pub enum Command {
    Play,
    Pause,
    Stop,
    Next,
    Previous,
    SeekTo(Duration),
    SetVolume(u8),
    SetReplayGain(ReplayGainMode),
    SetRandom(bool),
    SetLoop(LoopMode),
    PlayAt(usize),
    ReplaceQueue {
        uris: Vec<String>,
        position: usize,
        elapsed: Duration,
    },
    Append(Vec<String>),
    DeleteAt(usize),
    Move {
        from: usize,
        to: usize,
    },
    FetchArt {
        key: String,
        uri: String,
    },
    /// Re-reads the tags of the files below these directories.
    UpdateDatabase(Vec<String>),
}

impl Command {
    /// Art requests run in the background, so they are left out of the
    /// sent/processed bookkeeping.
    fn is_counted(&self) -> bool {
        !matches!(self, Command::FetchArt { .. })
    }

    fn changes_queue(&self) -> bool {
        matches!(
            self,
            Command::ReplaceQueue { .. }
                | Command::Append(_)
                | Command::DeleteAt(_)
                | Command::Move { .. }
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayerStatus {
    pub state: PlayState,
    pub volume: u8,
    pub elapsed: Duration,
    pub duration: Duration,
    pub position: Option<usize>,
    pub random: bool,
    pub loop_mode: LoopMode,
    pub updating_database: bool,
}

impl Default for PlayerStatus {
    fn default() -> Self {
        PlayerStatus {
            state: PlayState::Stopped,
            volume: 0,
            elapsed: Duration::ZERO,
            duration: Duration::ZERO,
            position: None,
            random: false,
            loop_mode: LoopMode::Off,
            updating_database: false,
        }
    }
}

/// `commands_processed` lets the UI tell whether a snapshot already reflects
/// the commands it has sent, or was taken before them.
#[derive(Debug)]
pub enum Event {
    Connected,
    Disconnected(String),
    Status {
        status: PlayerStatus,
        commands_processed: u64,
    },
    Queue {
        uris: Vec<String>,
        commands_processed: u64,
    },
    Library(Vec<Song>),
    ReplayGain(ReplayGainMode),
    Art {
        key: String,
        image: Option<egui::ColorImage>,
    },
    Error(String),
}

pub struct MpdHandle {
    commands: tokio_mpsc::UnboundedSender<Command>,
    events: mpsc::Receiver<Event>,
    commands_sent: u64,
}

impl MpdHandle {
    pub fn spawn(
        config: ConnectionConfig,
        art_cache_dir: PathBuf,
        ctx: egui::Context,
    ) -> MpdHandle {
        let (command_sender, command_receiver) = tokio_mpsc::unbounded_channel();
        let (event_sender, event_receiver) = mpsc::channel();

        let events = EventSender {
            sender: event_sender,
            ctx,
        };
        let worker = Worker {
            config,
            art_cache_dir,
            commands: command_receiver,
            events,
            commands_processed: 0,
        };

        std::thread::Builder::new()
            .name("mpd".to_string())
            .spawn(move || worker.run())
            .expect("failed to spawn the MPD thread");

        MpdHandle {
            commands: command_sender,
            events: event_receiver,
            commands_sent: 0,
        }
    }

    pub fn send(&mut self, command: Command) {
        if command.is_counted() {
            self.commands_sent += 1;
        }
        let _ = self.commands.send(command);
    }

    pub fn commands_sent(&self) -> u64 {
        self.commands_sent
    }

    pub fn poll_events(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }

    /// Blocks until MPD has stopped or the timeout passes, so the command
    /// is not lost when the app exits right after sending it.
    pub fn stop_and_wait(&mut self, timeout: Duration) {
        self.send(Command::Stop);
        let deadline = Instant::now() + timeout;
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            match self.events.recv_timeout(remaining) {
                Ok(Event::Status {
                    commands_processed, ..
                }) if commands_processed >= self.commands_sent => return,
                Ok(_) => {}
                Err(_) => return,
            }
        }
    }
}

/// Commands that can be sent from the command line, without opening a window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RemoteCommand {
    TogglePlayback,
    Next,
    Previous,
}

pub fn send_remote_command(
    config: &ConnectionConfig,
    command: RemoteCommand,
) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async {
        let (client, _connection_events) = connect(config).await?;
        match command {
            RemoteCommand::TogglePlayback => toggle_playback(&client).await?,
            RemoteCommand::Next => client.command(mpd_commands::Next).await?,
            RemoteCommand::Previous => client.command(mpd_commands::Previous).await?,
        }
        Ok(())
    })
}

async fn toggle_playback(client: &Client) -> Result<(), mpd_client::client::CommandError> {
    let status = client.command(mpd_commands::Status).await?;
    if status.state == PlayState::Playing {
        client.command(mpd_commands::SetPause(true)).await
    } else {
        client.command(mpd_commands::Play::current()).await
    }
}

async fn connect(config: &ConnectionConfig) -> anyhow::Result<(Client, ConnectionEvents)> {
    let password = config.password.as_deref();

    #[cfg(unix)]
    if config.is_socket_path() {
        let stream = tokio::net::UnixStream::connect(&config.host).await?;
        let connection = Client::connect_with_password_opt(stream, password).await?;
        return Ok(connection);
    }

    let address = (config.host.as_str(), config.port);
    let stream = tokio::net::TcpStream::connect(address)
        .await
        .with_context(|| format!("cannot reach MPD at {}", config.describe()))?;
    let connection = Client::connect_with_password_opt(stream, password).await?;
    Ok(connection)
}

#[derive(Clone)]
struct EventSender {
    sender: mpsc::Sender<Event>,
    ctx: egui::Context,
}

impl EventSender {
    fn send(&self, event: Event) {
        let _ = self.sender.send(event);
        self.ctx.request_repaint();
    }
}

struct Worker {
    config: ConnectionConfig,
    art_cache_dir: PathBuf,
    commands: tokio_mpsc::UnboundedReceiver<Command>,
    events: EventSender,
    commands_processed: u64,
}

impl Worker {
    fn run(mut self) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to start the tokio runtime");

        runtime.block_on(async {
            loop {
                match connect(&self.config).await {
                    Ok((client, connection_events)) => {
                        self.events.send(Event::Connected);
                        match self.run_session(&client, connection_events).await {
                            Ok(()) => return,
                            Err(error) => {
                                self.events.send(Event::Disconnected(format!("{error:#}")))
                            }
                        }
                    }
                    Err(error) => self.events.send(Event::Disconnected(format!("{error:#}"))),
                }

                if self.wait_before_reconnecting().await.is_err() {
                    return;
                }
            }
        });
    }

    /// Commands sent while disconnected are dropped so they don't replay on reconnect.
    /// Returns an error once the UI has gone away.
    async fn wait_before_reconnecting(&mut self) -> Result<(), ()> {
        let deadline = tokio::time::Instant::now() + RECONNECT_DELAY;
        loop {
            tokio::select! {
                command = self.commands.recv() => {
                    let Some(command) = command else {
                        return Err(());
                    };
                    if command.is_counted() {
                        self.commands_processed += 1;
                    }
                }
                _ = tokio::time::sleep_until(deadline) => return Ok(()),
            }
        }
    }

    /// Returns `Ok` when the UI has gone away, and an error when the connection is lost.
    async fn run_session(
        &mut self,
        client: &Client,
        mut connection_events: ConnectionEvents,
    ) -> anyhow::Result<()> {
        client
            .command(mpd_commands::SetBinaryLimit(BINARY_LIMIT_BYTES))
            .await?;
        self.send_library(client).await?;
        self.send_queue(client).await?;
        self.send_status(client).await?;
        self.send_replay_gain(client).await?;

        let mut status_poll = tokio::time::interval(STATUS_POLL_INTERVAL);

        loop {
            tokio::select! {
                command = self.commands.recv() => {
                    let Some(command) = command else {
                        return Ok(());
                    };
                    self.execute(client, command).await?;
                }
                event = connection_events.next() => {
                    match event {
                        Some(ConnectionEvent::SubsystemChange(subsystem)) => {
                            self.handle_subsystem_change(client, subsystem).await?;
                        }
                        Some(ConnectionEvent::ConnectionClosed(error)) => bail!(error),
                        None => bail!("connection closed"),
                    }
                }
                _ = status_poll.tick() => {
                    self.send_status(client).await?;
                }
            }
        }
    }

    async fn handle_subsystem_change(
        &mut self,
        client: &Client,
        subsystem: Subsystem,
    ) -> anyhow::Result<()> {
        match subsystem {
            Subsystem::Database => self.send_library(client).await,
            Subsystem::Queue => {
                self.send_queue(client).await?;
                self.send_status(client).await
            }
            Subsystem::Player | Subsystem::Mixer | Subsystem::Update => {
                self.send_status(client).await
            }
            Subsystem::Options => {
                self.send_status(client).await?;
                self.send_replay_gain(client).await
            }
            _ => Ok(()),
        }
    }

    /// MPD error responses (a missing file, a bad position) are reported and the session continues.
    /// Anything else means the connection is unusable.
    async fn execute(&mut self, client: &Client, command: Command) -> anyhow::Result<()> {
        if let Command::FetchArt { key, uri } = command {
            self.fetch_art_in_background(client, key, uri);
            return Ok(());
        }

        let changes_queue = command.changes_queue();
        let result = self.send_command(client, command).await;
        self.commands_processed += 1;

        match result {
            Ok(()) => {}
            Err(mpd_client::client::CommandError::ErrorResponse { error, .. }) => {
                self.events.send(Event::Error(error.message.to_string()));
            }
            Err(error) => return Err(error.into()),
        }

        if changes_queue {
            self.send_queue(client).await?;
        }
        self.send_status(client).await
    }

    async fn send_command(
        &self,
        client: &Client,
        command: Command,
    ) -> Result<(), mpd_client::client::CommandError> {
        match command {
            Command::Play => client.command(mpd_commands::Play::current()).await,
            Command::Pause => client.command(mpd_commands::SetPause(true)).await,
            Command::Stop => client.command(mpd_commands::Stop).await,
            Command::Next => client.command(mpd_commands::Next).await,
            Command::Previous => client.command(mpd_commands::Previous).await,
            Command::SeekTo(time) => {
                client
                    .command(mpd_commands::Seek(SeekMode::Absolute(time)))
                    .await
            }
            Command::SetVolume(volume) => client.command(mpd_commands::SetVolume(volume)).await,
            Command::SetReplayGain(mode) => {
                client.command(mpd_commands::SetReplayGainMode(mode)).await
            }
            Command::SetRandom(random) => client.command(mpd_commands::SetRandom(random)).await,
            Command::SetLoop(mode) => {
                let single = if mode == LoopMode::Track {
                    SingleMode::Enabled
                } else {
                    SingleMode::Disabled
                };
                client
                    .command(mpd_commands::SetRepeat(mode != LoopMode::Off))
                    .await?;
                client.command(mpd_commands::SetSingle(single)).await
            }
            Command::PlayAt(position) => {
                client
                    .command(mpd_commands::Play::song(SongPosition(position)))
                    .await
            }
            Command::ReplaceQueue {
                uris,
                position,
                elapsed,
            } => replace_queue(client, &uris, position, elapsed).await,
            Command::Append(uris) => append_to_queue(client, &uris).await,
            Command::DeleteAt(position) => {
                client
                    .command(mpd_commands::Delete::position(SongPosition(position)))
                    .await
            }
            Command::Move { from, to } => {
                let command =
                    mpd_commands::Move::position(SongPosition(from)).to_position(SongPosition(to));
                client.command(command).await
            }
            Command::UpdateDatabase(directories) => update_database(client, &directories).await,
            Command::FetchArt { .. } => Ok(()),
        }
    }

    fn fetch_art_in_background(&self, client: &Client, key: String, uri: String) {
        let client = client.clone();
        let cache_dir = self.art_cache_dir.clone();
        let events = self.events.clone();
        tokio::spawn(async move {
            let image = art::load(&client, &uri, &key, &cache_dir).await;
            events.send(Event::Art { key, image });
        });
    }

    async fn send_library(&self, client: &Client) -> anyhow::Result<()> {
        let songs = client.command(mpd_commands::ListAllIn::root()).await?;
        let songs = songs.iter().map(convert_song).collect();
        self.events.send(Event::Library(songs));
        Ok(())
    }

    async fn send_queue(&self, client: &Client) -> anyhow::Result<()> {
        let queue = client.command(mpd_commands::Queue).await?;
        let mut uris = Vec::new();
        for entry in queue {
            uris.push(entry.song.url);
        }
        self.events.send(Event::Queue {
            uris,
            commands_processed: self.commands_processed,
        });
        Ok(())
    }

    async fn send_status(&self, client: &Client) -> anyhow::Result<()> {
        let status = client.command(mpd_commands::Status).await?;
        let status = PlayerStatus {
            state: status.state,
            volume: status.volume,
            elapsed: status.elapsed.unwrap_or_default(),
            duration: status.duration.unwrap_or_default(),
            position: status.current_song.map(|(position, _id)| position.0),
            random: status.random,
            loop_mode: LoopMode::from_mpd(status.repeat, status.single),
            updating_database: status.update_job.is_some(),
        };
        self.events.send(Event::Status {
            status,
            commands_processed: self.commands_processed,
        });
        Ok(())
    }

    async fn send_replay_gain(&self, client: &Client) -> anyhow::Result<()> {
        let status = client.command(mpd_commands::ReplayGainStatus).await?;
        self.events.send(Event::ReplayGain(status.mode));
        Ok(())
    }
}

/// Clearing and re-adding happens in one command list so other clients never see a half-built queue.
async fn replace_queue(
    client: &Client,
    uris: &[String],
    position: usize,
    elapsed: Duration,
) -> Result<(), mpd_client::client::CommandError> {
    let mut commands = RawCommandList::new(mpd_commands::ClearQueue.command());
    for uri in uris {
        commands.add(mpd_commands::Add::uri(uri).command());
    }
    client.raw_command_list(commands).await?;

    if uris.is_empty() {
        return Ok(());
    }
    if elapsed.is_zero() {
        client
            .command(mpd_commands::Play::song(SongPosition(position)))
            .await
    } else {
        client
            .command(mpd_commands::SeekTo(SongPosition(position).into(), elapsed))
            .await
    }
}

async fn append_to_queue(
    client: &Client,
    uris: &[String],
) -> Result<(), mpd_client::client::CommandError> {
    let mut adds = Vec::new();
    for uri in uris {
        adds.push(mpd_commands::Add::uri(uri));
    }
    if adds.is_empty() {
        return Ok(());
    }
    client.command_list(adds).await?;
    Ok(())
}

/// An empty directory is the music root, which MPD updates when no path is given.
async fn update_database(
    client: &Client,
    directories: &[String],
) -> Result<(), mpd_client::client::CommandError> {
    let mut updates = Vec::new();
    for directory in directories {
        if directory.is_empty() {
            updates.push(mpd_commands::Update::new());
        } else {
            updates.push(mpd_commands::Update::new().uri(directory));
        }
    }
    if updates.is_empty() {
        return Ok(());
    }
    client.command_list(updates).await?;
    Ok(())
}

fn convert_song(song: &mpd_client::responses::Song) -> Song {
    let first_tag = |tag: Tag| -> String {
        match song.tags.get(&tag).and_then(|values| values.first()) {
            Some(value) => value.clone(),
            None => String::new(),
        }
    };
    let leading_number = |tag: Tag| -> Option<u32> {
        let value = first_tag(tag);
        let number = value.split('/').next()?;
        number.trim().parse().ok()
    };

    Song {
        uri: song.url.clone(),
        title: first_tag(Tag::Title),
        artist: song.artists().join(", "),
        album_artist: song.album_artists().join(", "),
        album: first_tag(Tag::Album),
        genre: first_tag(Tag::Genre),
        date: first_tag(Tag::Date),
        track: leading_number(Tag::Track),
        disc: leading_number(Tag::Disc),
        duration: song.duration,
    }
}

/// Needs a running MPD with at least two songs: `MPD_PORT=6600 cargo test -- --ignored`.
/// It replaces that MPD's queue.
#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for<T>(handle: &MpdHandle, mut pick: impl FnMut(Event) -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            for event in handle.poll_events() {
                if let Some(found) = pick(event) {
                    return found;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("timed out waiting for an MPD event");
    }

    #[test]
    #[ignore]
    fn drives_a_real_mpd() {
        let cache_dir = std::env::temp_dir().join(format!("mewsic-art-{}", std::process::id()));
        let mut handle = MpdHandle::spawn(
            ConnectionConfig::from_env(),
            cache_dir.clone(),
            egui::Context::default(),
        );

        let songs = wait_for(&handle, |event| match event {
            Event::Library(songs) => Some(songs),
            Event::Disconnected(reason) => panic!("{reason}"),
            _ => None,
        });
        let mut uris = Vec::new();
        for song in &songs {
            let in_hidden_directory = song.uri.split('/').any(|part| part.starts_with('.'));
            if !in_hidden_directory && uris.len() < 2 {
                uris.push(song.uri.clone());
            }
        }
        assert_eq!(uris.len(), 2, "need at least two songs");

        handle.send(Command::ReplaceQueue {
            uris: uris.clone(),
            position: 1,
            elapsed: Duration::ZERO,
        });
        let sent = handle.commands_sent();
        let queue = wait_for(&handle, |event| match event {
            Event::Queue {
                uris,
                commands_processed,
            } if commands_processed >= sent => Some(uris),
            _ => None,
        });
        assert_eq!(queue, uris);

        let status = wait_for(&handle, |event| match event {
            Event::Status {
                status,
                commands_processed,
            } if commands_processed >= sent => Some(status),
            _ => None,
        });
        assert_eq!(status.position, Some(1));
        assert_eq!(status.state, PlayState::Playing);

        handle.send(Command::SetReplayGain(ReplayGainMode::Album));
        let mode = wait_for(&handle, |event| match event {
            Event::ReplayGain(mode) if mode == ReplayGainMode::Album => Some(mode),
            _ => None,
        });
        assert_eq!(mode, ReplayGainMode::Album);
        handle.send(Command::SetReplayGain(ReplayGainMode::Off));

        handle.send(Command::FetchArt {
            key: "test".to_string(),
            uri: uris[0].clone(),
        });
        let has_art = wait_for(&handle, |event| match event {
            Event::Art { image, .. } => Some(image.is_some()),
            _ => None,
        });
        println!("album art found for {}: {has_art}", uris[0]);

        handle.send(Command::SetRandom(true));
        handle.send(Command::SetLoop(LoopMode::Track));
        let sent = handle.commands_sent();
        let status = wait_for(&handle, |event| match event {
            Event::Status {
                status,
                commands_processed,
            } if commands_processed >= sent => Some(status),
            _ => None,
        });
        assert!(status.random);
        assert_eq!(status.loop_mode, LoopMode::Track);
        handle.send(Command::SetRandom(false));
        handle.send(Command::SetLoop(LoopMode::Off));

        handle.stop_and_wait(Duration::from_secs(5));
        let status = wait_for(&handle, |event| match event {
            Event::Status { status, .. } => Some(status),
            _ => None,
        });
        assert_eq!(status.state, PlayState::Stopped);
        let _ = std::fs::remove_dir_all(cache_dir);
    }
}
