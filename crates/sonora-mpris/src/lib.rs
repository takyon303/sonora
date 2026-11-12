//! Desktop media controls through MPRIS.
//!
//! A Linux shell finds whichever process owns `org.mpris.MediaPlayer2.*` on the
//! session bus and turns it into media keys, lock-screen controls and a panel
//! widget. Sonora publishes the state its transport bar already keeps and
//! accepts the handful of commands those surfaces send back.
//!
//! The bus work runs on its own thread with its own current-thread runtime, so
//! a shell that is slow, absent or crashes never blocks the UI. Every update is
//! a snapshot: the UI pushes one on each poll, the publisher emits only the
//! properties that actually changed, and commands wait in a queue the UI drains
//! on the same poll. Nothing here owns playback; it is a view and a remote.

/// The bus name a shell looks for. Sonora only ever publishes one instance, so
/// the name carries no pid suffix.
pub const BUS_NAME: &str = "org.mpris.MediaPlayer2.sonora";

/// The object path every MPRIS player serves.
pub const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";

/// The playback state as MPRIS spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackStatus {
    #[default]
    Stopped,
    Playing,
    Paused,
}

impl PlaybackStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "Stopped",
            Self::Playing => "Playing",
            Self::Paused => "Paused",
        }
    }
}

/// MPRIS `LoopStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoopStatus {
    #[default]
    None,
    Track,
    Playlist,
}

impl LoopStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Track => "Track",
            Self::Playlist => "Playlist",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "None" => Some(Self::None),
            "Track" => Some(Self::Track),
            "Playlist" => Some(Self::Playlist),
            _ => None,
        }
    }
}

/// What the shell shows for the current track.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NowPlaying {
    pub track_id: i64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub length_ms: u64,
}

/// One snapshot of everything MPRIS can report.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerState {
    pub status: PlaybackStatus,
    pub track: Option<NowPlaying>,
    pub position_ms: u64,
    pub volume: f64,
    pub can_go_next: bool,
    pub can_go_previous: bool,
    pub can_seek: bool,
    pub shuffle: bool,
    pub repeat: LoopStatus,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            status: PlaybackStatus::Stopped,
            track: None,
            position_ms: 0,
            volume: 1.0,
            can_go_next: false,
            can_go_previous: false,
            can_seek: false,
            shuffle: false,
            repeat: LoopStatus::None,
        }
    }
}

/// Something a shell asked for. Applying these is the UI's job, because only it
/// owns the engine and the queue.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    /// Offset in microseconds, positive or negative.
    SeekBy(i64),
    /// Absolute position in microseconds.
    SeekTo(u64),
    SetVolume(f64),
    SetShuffle(bool),
    SetRepeat(LoopStatus),
    /// Bring the window to the front.
    Raise,
}

/// A guard that ignores a poisoned lock.
///
/// A panic on the publisher thread must not take playback commands down with
/// it, and every piece of state behind these locks is plain data.
#[cfg(target_os = "linux")]
pub(crate) fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{lock, Command, LoopStatus, NowPlaying, PlayerState};
    use crate::{BUS_NAME, OBJECT_PATH};
    use std::collections::HashMap;
    use std::sync::mpsc as std_mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::mpsc as tokio_mpsc;
    use zbus::interface;
    use zbus::zvariant::{ObjectPath, OwnedValue, Value};

    const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";

    /// State shared with the interface methods that run on the bus thread.
    struct Shared {
        state: Mutex<PlayerState>,
        commands: Mutex<Vec<Command>>,
    }

    impl Shared {
        fn push(&self, command: Command) {
            lock(&self.commands).push(command);
        }
    }

    /// `org.mpris.MediaPlayer2`, the identity half of the protocol.
    struct Root {
        shared: Arc<Shared>,
    }

    #[interface(name = "org.mpris.MediaPlayer2")]
    impl Root {
        /// Bring Sonora's window to the front.
        async fn raise(&self) {
            self.shared.push(Command::Raise);
        }

        /// Required by the interface. Sonora saves and closes through its own
        /// window lifecycle, so a remote quit would skip the save; `CanQuit`
        /// reports that honestly.
        async fn quit(&self) {}

        #[zbus(property)]
        fn identity(&self) -> &str {
            "Sonora"
        }

        #[zbus(property)]
        fn desktop_entry(&self) -> &str {
            "sonora"
        }

        #[zbus(property)]
        fn can_quit(&self) -> bool {
            false
        }

        #[zbus(property)]
        fn can_raise(&self) -> bool {
            true
        }

        #[zbus(property)]
        fn has_track_list(&self) -> bool {
            false
        }

        #[zbus(property)]
        fn supported_uri_schemes(&self) -> Vec<String> {
            Vec::new()
        }

        #[zbus(property)]
        fn supported_mime_types(&self) -> Vec<String> {
            Vec::new()
        }
    }

    /// `org.mpris.MediaPlayer2.Player`, the transport half.
    struct Player {
        shared: Arc<Shared>,
    }

    #[interface(name = "org.mpris.MediaPlayer2.Player")]
    impl Player {
        async fn next(&self) {
            self.shared.push(Command::Next);
        }

        async fn previous(&self) {
            self.shared.push(Command::Previous);
        }

        async fn pause(&self) {
            self.shared.push(Command::Pause);
        }

        async fn play_pause(&self) {
            self.shared.push(Command::PlayPause);
        }

        async fn stop(&self) {
            self.shared.push(Command::Stop);
        }

        async fn play(&self) {
            self.shared.push(Command::Play);
        }

        /// Relative seek in microseconds.
        async fn seek(&self, offset: i64) {
            self.shared.push(Command::SeekBy(offset));
        }

        /// Absolute seek, only for the track the shell thinks is playing.
        async fn set_position(&self, track_id: ObjectPath<'_>, position: i64) {
            let current = lock(&self.shared.state)
                .track
                .as_ref()
                .map(|track| track_path(track.track_id));
            if current.as_deref() != Some(track_id.as_str()) {
                return;
            }
            self.shared.push(Command::SeekTo(position.max(0) as u64));
        }

        #[zbus(property)]
        fn playback_status(&self) -> String {
            lock(&self.shared.state).status.as_str().to_string()
        }

        #[zbus(property)]
        fn loop_status(&self) -> String {
            lock(&self.shared.state).repeat.as_str().to_string()
        }

        #[zbus(property)]
        fn set_loop_status(&mut self, value: String) {
            let Some(mode) = LoopStatus::parse(&value) else {
                return;
            };
            lock(&self.shared.state).repeat = mode;
            self.shared.push(Command::SetRepeat(mode));
        }

        #[zbus(property)]
        fn rate(&self) -> f64 {
            1.0
        }

        #[zbus(property)]
        fn minimum_rate(&self) -> f64 {
            1.0
        }

        #[zbus(property)]
        fn maximum_rate(&self) -> f64 {
            1.0
        }

        #[zbus(property)]
        fn shuffle(&self) -> bool {
            lock(&self.shared.state).shuffle
        }

        #[zbus(property)]
        fn set_shuffle(&mut self, value: bool) {
            lock(&self.shared.state).shuffle = value;
            self.shared.push(Command::SetShuffle(value));
        }

        #[zbus(property)]
        fn metadata(&self) -> HashMap<String, OwnedValue> {
            let state = lock(&self.shared.state);
            metadata_map(state.track.as_ref())
        }

        #[zbus(property)]
        fn volume(&self) -> f64 {
            lock(&self.shared.state).volume
        }

        #[zbus(property)]
        fn set_volume(&mut self, value: f64) {
            let volume = value.clamp(0.0, 1.0);
            lock(&self.shared.state).volume = volume;
            self.shared.push(Command::SetVolume(volume));
        }

        #[zbus(property)]
        fn position(&self) -> i64 {
            (lock(&self.shared.state).position_ms as i64) * 1_000
        }

        #[zbus(property)]
        fn can_go_next(&self) -> bool {
            lock(&self.shared.state).can_go_next
        }

        #[zbus(property)]
        fn can_go_previous(&self) -> bool {
            lock(&self.shared.state).can_go_previous
        }

        #[zbus(property)]
        fn can_play(&self) -> bool {
            true
        }

        #[zbus(property)]
        fn can_pause(&self) -> bool {
            true
        }

        #[zbus(property)]
        fn can_seek(&self) -> bool {
            lock(&self.shared.state).can_seek
        }

        #[zbus(property)]
        fn can_control(&self) -> bool {
            true
        }
    }

    /// The `mpris:trackid` for a track. `0` means "nothing playing", which MPRIS
    /// spells as `NoTrack`.
    fn track_path(track_id: i64) -> String {
        if track_id <= 0 {
            NO_TRACK.to_string()
        } else {
            format!("/org/sonora/track/{track_id}")
        }
    }

    /// The metadata map in protocol order, before the values are boxed. Kept
    /// separate so a test can read the values without going through the bus.
    fn metadata_entries(track: Option<&NowPlaying>) -> Vec<(&'static str, Value<'static>)> {
        let Some(track) = track else {
            return vec![("mpris:trackid", path_value(NO_TRACK))];
        };

        vec![
            ("mpris:trackid", path_value(&track_path(track.track_id))),
            (
                "mpris:length",
                Value::from((track.length_ms as i64) * 1_000),
            ),
            ("xesam:title", Value::from(track.title.clone())),
            ("xesam:artist", Value::from(vec![track.artist.clone()])),
            ("xesam:album", Value::from(track.album.clone())),
        ]
    }

    fn path_value(path: &str) -> Value<'static> {
        match ObjectPath::try_from(path.to_owned()) {
            Ok(path) => Value::from(path),
            // Only the constants above reach this, so a failure cannot happen;
            // falling back to the empty path keeps the interface well-formed.
            Err(_) => Value::from(ObjectPath::try_from("/").expect("root path")),
        }
    }

    fn metadata_map(track: Option<&NowPlaying>) -> HashMap<String, OwnedValue> {
        metadata_entries(track)
            .into_iter()
            .filter_map(|(name, value)| Some((name.to_string(), OwnedValue::try_from(value).ok()?)))
            .collect()
    }

    /// Which properties a snapshot changed, so the publisher emits the smallest
    /// signal it can. Position is deliberately absent: MPRIS expects clients to
    /// poll it, and a signal four times a second is noise.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Changed {
        PlaybackStatus,
        Metadata,
        CanGoNext,
        CanGoPrevious,
        CanSeek,
        Shuffle,
        LoopStatus,
        Volume,
    }

    fn changed_between(previous: &PlayerState, next: &PlayerState) -> Vec<Changed> {
        let mut changed = Vec::new();
        if previous.status != next.status {
            changed.push(Changed::PlaybackStatus);
        }
        if previous.track != next.track {
            changed.push(Changed::Metadata);
        }
        if previous.can_go_next != next.can_go_next {
            changed.push(Changed::CanGoNext);
        }
        if previous.can_go_previous != next.can_go_previous {
            changed.push(Changed::CanGoPrevious);
        }
        if previous.can_seek != next.can_seek {
            changed.push(Changed::CanSeek);
        }
        if previous.shuffle != next.shuffle {
            changed.push(Changed::Shuffle);
        }
        if previous.repeat != next.repeat {
            changed.push(Changed::LoopStatus);
        }
        if (previous.volume - next.volume).abs() > f64::EPSILON {
            changed.push(Changed::Volume);
        }
        changed
    }

    async fn emit(connection: &zbus::Connection, changed: Changed) {
        let Ok(interface) = connection
            .object_server()
            .interface::<_, Player>(OBJECT_PATH)
            .await
        else {
            return;
        };
        let emitter = interface.signal_emitter();
        let player = interface.get().await;
        let result = match changed {
            Changed::PlaybackStatus => player.playback_status_changed(emitter).await,
            Changed::Metadata => player.metadata_changed(emitter).await,
            Changed::CanGoNext => player.can_go_next_changed(emitter).await,
            Changed::CanGoPrevious => player.can_go_previous_changed(emitter).await,
            Changed::CanSeek => player.can_seek_changed(emitter).await,
            Changed::Shuffle => player.shuffle_changed(emitter).await,
            Changed::LoopStatus => player.loop_status_changed(emitter).await,
            Changed::Volume => player.volume_changed(emitter).await,
        };
        if let Err(err) = result {
            log::debug!("MPRIS could not signal a change: {err}");
        }
    }

    fn serve(
        shared: Arc<Shared>,
        mut updates: tokio_mpsc::UnboundedReceiver<PlayerState>,
        ready: std_mpsc::Sender<Result<(), String>>,
    ) {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(err) => {
                let _ = ready.send(Err(format!("no runtime: {err}")));
                return;
            }
        };

        runtime.block_on(async move {
            let builder = match zbus::connection::Builder::session() {
                Ok(builder) => builder,
                Err(err) => {
                    let _ = ready.send(Err(format!("no session bus: {err}")));
                    return;
                }
            };

            let published = builder
                .name(BUS_NAME)
                .and_then(|builder| {
                    builder.serve_at(
                        OBJECT_PATH,
                        Root {
                            shared: shared.clone(),
                        },
                    )
                })
                .and_then(|builder| {
                    builder.serve_at(
                        OBJECT_PATH,
                        Player {
                            shared: shared.clone(),
                        },
                    )
                });

            let connection = match published {
                Ok(builder) => match builder.build().await {
                    Ok(connection) => connection,
                    Err(err) => {
                        let _ = ready.send(Err(format!("could not publish: {err}")));
                        return;
                    }
                },
                Err(err) => {
                    let _ = ready.send(Err(format!("could not publish: {err}")));
                    return;
                }
            };

            if ready.send(Ok(())).is_err() {
                return;
            }

            let mut previous = lock(&shared.state).clone();
            while let Some(next) = updates.recv().await {
                let changed = changed_between(&previous, &next);
                *lock(&shared.state) = next.clone();
                previous = next;
                for property in changed {
                    emit(&connection, property).await;
                }
            }
        });
    }

    /// Handle to a published player. Dropping it closes the update channel and
    /// lets the bus thread exit.
    pub struct Mpris {
        shared: Arc<Shared>,
        updates: tokio_mpsc::UnboundedSender<PlayerState>,
    }

    impl Mpris {
        /// Publish a player on the session bus.
        ///
        /// Returns `None` when there is no session bus, another Sonora already
        /// owns the name, or the bus did not answer in time. Media controls are
        /// an extra, so a failure is reported once and never blocks the UI.
        pub fn start(initial: PlayerState) -> Option<Self> {
            let shared = Arc::new(Shared {
                state: Mutex::new(initial),
                commands: Mutex::new(Vec::new()),
            });
            let (updates, receiver) = tokio_mpsc::unbounded_channel();
            let (ready, published) = std_mpsc::channel();

            let thread_shared = shared.clone();
            let thread = std::thread::Builder::new()
                .name("sonora-mpris".to_string())
                .spawn(move || serve(thread_shared, receiver, ready))
                .ok()?;

            match published.recv_timeout(Duration::from_millis(750)) {
                Ok(Ok(())) => {
                    // The thread lives until the last sender is dropped, which
                    // happens with this handle; keep the join handle detached.
                    drop(thread);
                    Some(Self { shared, updates })
                }
                Ok(Err(err)) => {
                    log::info!("media controls unavailable: {err}");
                    None
                }
                Err(_) => {
                    log::info!("media controls did not publish in time");
                    None
                }
            }
        }

        /// Publish a fresh snapshot. Cheap enough to call on every UI poll; the
        /// publisher only signals what changed.
        pub fn update(&self, state: PlayerState) {
            let _ = self.updates.send(state);
        }

        /// Take everything the shell asked for since the last call.
        pub fn take_commands(&self) -> Vec<Command> {
            std::mem::take(&mut *lock(&self.shared.commands))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::PlaybackStatus;

        fn track() -> NowPlaying {
            NowPlaying {
                track_id: 7,
                title: "Opening".to_string(),
                artist: "Nils Frahm".to_string(),
                album: "Spaces".to_string(),
                length_ms: 204_000,
            }
        }

        fn entry<'a>(
            entries: &'a [(&'static str, Value<'static>)],
            name: &str,
        ) -> &'a Value<'static> {
            &entries
                .iter()
                .find(|(key, _)| *key == name)
                .unwrap_or_else(|| panic!("metadata has no {name}"))
                .1
        }

        #[test]
        fn metadata_uses_the_value_types_mpris_clients_expect() {
            let entries = metadata_entries(Some(&track()));
            assert!(matches!(
                entry(&entries, "mpris:trackid"),
                Value::ObjectPath(_)
            ));
            assert!(matches!(entry(&entries, "mpris:length"), Value::I64(_)));
            assert!(matches!(entry(&entries, "xesam:title"), Value::Str(_)));
            assert!(matches!(entry(&entries, "xesam:artist"), Value::Array(_)));
            assert!(matches!(entry(&entries, "xesam:album"), Value::Str(_)));

            match entry(&entries, "mpris:length") {
                Value::I64(length) => assert_eq!(*length, 204_000_000),
                other => panic!("length was {other:?}"),
            }
            match entry(&entries, "mpris:trackid") {
                Value::ObjectPath(path) => assert_eq!(path.as_str(), "/org/sonora/track/7"),
                other => panic!("track id was {other:?}"),
            }
        }

        #[test]
        fn nothing_playing_still_carries_the_no_track_id() {
            let entries = metadata_entries(None);
            assert_eq!(entries.len(), 1);
            match entry(&entries, "mpris:trackid") {
                Value::ObjectPath(path) => assert_eq!(path.as_str(), NO_TRACK),
                other => panic!("track id was {other:?}"),
            }
        }

        #[test]
        fn only_changed_properties_are_signalled() {
            let playing = PlayerState {
                status: PlaybackStatus::Playing,
                can_seek: true,
                repeat: LoopStatus::Track,
                volume: 0.4,
                ..PlayerState::default()
            };
            assert!(changed_between(&playing, &playing).is_empty());
            let changed = changed_between(&PlayerState::default(), &playing);
            assert_eq!(
                changed,
                vec![
                    Changed::PlaybackStatus,
                    Changed::CanSeek,
                    Changed::LoopStatus,
                    Changed::Volume,
                ]
            );
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::{Command, PlayerState};

    /// MPRIS is a Linux protocol; elsewhere this is a compile-time no-op so the
    /// rest of the workspace stays platform-neutral.
    pub struct Mpris;

    impl Mpris {
        pub fn start(_initial: PlayerState) -> Option<Self> {
            None
        }

        pub fn update(&self, _state: PlayerState) {}

        pub fn take_commands(&self) -> Vec<Command> {
            Vec::new()
        }
    }
}

pub use imp::Mpris;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_status_round_trips_through_the_protocol_spelling() {
        for status in [LoopStatus::None, LoopStatus::Track, LoopStatus::Playlist] {
            assert_eq!(LoopStatus::parse(status.as_str()), Some(status));
        }
        assert_eq!(LoopStatus::parse("Something"), None);
    }

    #[test]
    fn playback_status_uses_the_mpris_words() {
        assert_eq!(PlaybackStatus::Playing.as_str(), "Playing");
        assert_eq!(PlaybackStatus::Paused.as_str(), "Paused");
        assert_eq!(PlaybackStatus::Stopped.as_str(), "Stopped");
    }

    #[test]
    fn a_snapshot_defaults_to_a_stopped_player() {
        let state = PlayerState::default();
        assert_eq!(state.status, PlaybackStatus::Stopped);
        assert_eq!(state.volume, 1.0);
        assert!(state.track.is_none());
    }
}
