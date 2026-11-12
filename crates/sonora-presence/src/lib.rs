//! Discord Rich Presence.
//!
//! Discord's presence is a small, write-only IPC protocol: a length-prefixed
//! frame on a socket the Discord desktop app owns. The window hands this crate
//! a snapshot of the transport; a worker thread owns the socket, retries when
//! Discord starts after Sonora, and publishes nothing at all unless a track is
//! actually playing. A build without the socket, or a machine without Discord,
//! simply never connects, and the window is never held up by it.
//!
//! Every published activity names the build (`Sonora pre-release`), because the
//! application name Discord shows comes from the Discord application it was
//! registered as, not from this program.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

/// The second line of the presence. The first line is the application name
/// Discord knows, which cannot be set from here.
pub const PRE_RELEASE_LABEL: &str = "Sonora pre-release";

/// Discord's own limit for the details and state lines.
const LINE_LIMIT: usize = 128;

/// How often a worker without a connection tries again, so Discord may start
/// long after Sonora does.
const RECONNECT_INTERVAL: Duration = Duration::from_secs(15);

const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;

/// What the transport is doing right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub position_ms: i64,
    pub duration_ms: i64,
    /// A presence is only shown while this is true.
    pub playing: bool,
}

/// The activity Discord should show for this snapshot, or `None` when nothing
/// should be shown at all.
pub fn activity(track: &NowPlaying, now_unix: i64) -> Option<Value> {
    if !track.playing {
        return None;
    }
    let title = track.title.trim();
    let artist = track.artist.trim();
    if title.is_empty() && artist.is_empty() {
        return None;
    }
    let details = match (title.is_empty(), artist.is_empty()) {
        (false, false) => format!("{title} — {artist}"),
        (false, true) => title.to_string(),
        (true, false) => artist.to_string(),
        (true, true) => return None,
    };

    let mut activity = json!({
        "details": shorten(&details, LINE_LIMIT),
        "state": PRE_RELEASE_LABEL,
    });
    // Absolute timestamps, so the progress Discord draws stays correct even
    // though the presence is only republished when something changes.
    if track.duration_ms > 0 {
        let started = now_unix - track.position_ms.max(0) / 1_000;
        activity["timestamps"] = json!({
            "start": started,
            "end": started + track.duration_ms / 1_000,
        });
    }
    Some(activity)
}

fn shorten(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut shortened: String = text.chars().take(limit.saturating_sub(1)).collect();
    shortened.push('…');
    shortened
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

trait Transport: Read + Write + Send {}
impl<T: Read + Write + Send> Transport for T {}

/// A live IPC connection to the Discord desktop app.
struct Client {
    stream: Box<dyn Transport>,
}

impl Client {
    fn new(stream: Box<dyn Transport>) -> Self {
        Self { stream }
    }

    /// The first frame Discord expects: this is who is talking.
    fn handshake(&mut self, app_id: &str) -> std::io::Result<()> {
        self.send(OP_HANDSHAKE, &json!({ "v": 1, "client_id": app_id }))
    }

    /// Show this track, or clear the presence when there is nothing to show.
    fn show(&mut self, track: Option<&NowPlaying>) -> std::io::Result<()> {
        static NONCE: AtomicU64 = AtomicU64::new(1);
        let activity = track.and_then(|track| activity(track, unix_now()));
        let payload = json!({
            "cmd": "SET_ACTIVITY",
            "args": { "pid": std::process::id(), "activity": activity },
            "nonce": format!("{}", NONCE.fetch_add(1, Ordering::Relaxed)),
        });
        self.send(OP_FRAME, &payload)
    }

    fn send(&mut self, opcode: u32, payload: &Value) -> std::io::Result<()> {
        let text = payload.to_string();
        let mut frame = Vec::with_capacity(8 + text.len());
        frame.extend_from_slice(&opcode.to_le_bytes());
        frame.extend_from_slice(&(text.len() as u32).to_le_bytes());
        frame.extend_from_slice(text.as_bytes());
        self.stream.write_all(&frame)?;
        self.stream.flush()
    }
}

enum Command {
    Connect(String),
    Disconnect,
    Publish(Option<NowPlaying>),
    Shutdown,
}

/// A handle to the presence worker. Cheap to keep around even when the feature
/// is off: nothing is sent and nothing is connected until `connect` is called.
pub struct Presence {
    tx: Sender<Command>,
    connected: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Presence {
    pub fn start() -> Self {
        Self::start_on(socket_paths())
    }

    fn start_on(paths: Vec<PathBuf>) -> Self {
        let (tx, rx) = mpsc::channel();
        let connected = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&connected);
        let worker = std::thread::Builder::new()
            .name("sonora-presence".to_string())
            .spawn(move || run(rx, paths, flag))
            .ok();
        Self {
            tx,
            connected,
            worker,
        }
    }

    /// Publish for this Discord application, or stop entirely with `None`.
    pub fn connect(&self, app_id: Option<&str>) {
        let command = match app_id.map(str::trim).filter(|id| !id.is_empty()) {
            Some(id) => Command::Connect(id.to_string()),
            None => Command::Disconnect,
        };
        let _ = self.tx.send(command);
    }

    /// What is playing now; `None` clears the presence.
    pub fn publish(&self, track: Option<NowPlaying>) {
        let _ = self.tx.send(Command::Publish(track));
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run(rx: Receiver<Command>, paths: Vec<PathBuf>, connected: Arc<AtomicBool>) {
    let mut app_id: Option<String> = None;
    let mut current: Option<NowPlaying> = None;
    let mut client: Option<Client> = None;
    // What the live connection has already been told, so nothing is written
    // twice and a republished presence never spams Discord.
    let mut told: Option<Option<NowPlaying>> = None;

    loop {
        let command = match rx.recv_timeout(RECONNECT_INTERVAL) {
            Ok(command) => Some(command),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        let mut stop = false;
        match command {
            Some(Command::Connect(id)) => {
                if app_id.as_deref() != Some(id.as_str()) {
                    app_id = Some(id);
                    client = None;
                    told = None;
                }
            }
            Some(Command::Disconnect) => {
                app_id = None;
                client = None;
                told = None;
            }
            Some(Command::Publish(state)) => current = state,
            Some(Command::Shutdown) => stop = true,
            None => {}
        }
        if stop {
            if let Some(active) = &mut client {
                let _ = active.show(None);
            }
            break;
        }

        if let (Some(id), true) = (app_id.as_deref(), client.is_none()) {
            client = connect(&paths, id);
            // A fresh connection shows nothing until told otherwise, so a clear
            // needs no frame of its own.
            told = client.as_ref().map(|_| None);
        }
        if app_id.is_none() {
            client = None;
        }

        if let Some(active) = &mut client {
            let wanted = current.clone();
            if told.as_ref() != Some(&wanted) {
                match active.show(wanted.as_ref()) {
                    Ok(()) => told = Some(wanted),
                    Err(err) => {
                        log::debug!("Discord presence could not be updated: {err}");
                        client = None;
                        told = None;
                    }
                }
            }
        }
        connected.store(client.is_some(), Ordering::Relaxed);
    }

    connected.store(false, Ordering::Relaxed);
}

#[cfg(unix)]
fn connect(paths: &[PathBuf], app_id: &str) -> Option<Client> {
    use std::os::unix::net::UnixStream;

    for path in paths {
        let Ok(stream) = UnixStream::connect(path) else {
            continue;
        };
        let mut client = Client::new(Box::new(stream));
        match client.handshake(app_id) {
            Ok(()) => return Some(client),
            Err(err) => log::debug!("Discord refused the handshake at {}: {err}", path.display()),
        }
    }
    log::debug!("no Discord socket found; presence stays off");
    None
}

#[cfg(windows)]
fn connect(paths: &[PathBuf], app_id: &str) -> Option<Client> {
    for path in paths {
        let Ok(file) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
        else {
            continue;
        };
        let mut client = Client::new(Box::new(file));
        match client.handshake(app_id) {
            Ok(()) => return Some(client),
            Err(err) => log::debug!("Discord refused the handshake at {}: {err}", path.display()),
        }
    }
    log::debug!("no Discord pipe found; presence stays off");
    None
}

#[cfg(not(any(unix, windows)))]
fn connect(_paths: &[PathBuf], _app_id: &str) -> Option<Client> {
    None
}

/// Where Discord's IPC socket may be, in the order it is worth trying.
#[cfg(unix)]
fn socket_paths() -> Vec<PathBuf> {
    use std::collections::BTreeSet;

    let mut bases = BTreeSet::new();
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        let runtime = PathBuf::from(runtime);
        // Flatpak keeps its own runtime directory inside the sandbox.
        bases.insert(runtime.join("app/com.discordapp.Discord"));
        bases.insert(runtime.join("app/com.discordapp.DiscordCanary"));
        bases.insert(runtime);
    }
    bases.insert(std::env::temp_dir());
    bases.insert(PathBuf::from("/tmp"));
    bases.insert(PathBuf::from("/tmp/snap.discord"));
    bases.insert(PathBuf::from("/tmp/snap.discord-canary"));

    let mut paths = Vec::new();
    for base in bases {
        for index in 0..10 {
            paths.push(base.join(format!("discord-ipc-{index}")));
        }
    }
    paths
}

#[cfg(windows)]
fn socket_paths() -> Vec<PathBuf> {
    (0..10)
        .map(|index| PathBuf::from(format!(r"\\.\pipe\discord-ipc-{index}")))
        .collect()
}

#[cfg(not(any(unix, windows)))]
fn socket_paths() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing() -> NowPlaying {
        NowPlaying {
            title: "Xtal".to_string(),
            artist: "Aphex Twin".to_string(),
            album: Some("SAW 85-92".to_string()),
            position_ms: 65_000,
            duration_ms: 293_000,
            playing: true,
        }
    }

    #[test]
    fn a_playing_track_names_the_track_and_this_build() {
        let activity = activity(&playing(), 1_700_000_000).expect("activity");
        assert_eq!(activity["details"], "Xtal — Aphex Twin");
        assert_eq!(activity["state"], PRE_RELEASE_LABEL);
        assert_eq!(activity["timestamps"]["start"], 1_699_999_935);
        assert_eq!(activity["timestamps"]["end"], 1_700_000_228);
    }

    #[test]
    fn nothing_is_published_while_paused_or_stopped() {
        let mut track = playing();
        track.playing = false;
        assert_eq!(activity(&track, 1_700_000_000), None);
    }

    #[test]
    fn a_track_with_no_name_publishes_nothing() {
        let mut track = playing();
        track.title = "  ".to_string();
        track.artist = String::new();
        assert_eq!(activity(&track, 1_700_000_000), None);
    }

    #[test]
    fn an_unknown_duration_publishes_no_progress_bar() {
        let mut track = playing();
        track.duration_ms = 0;
        let activity = activity(&track, 1_700_000_000).expect("activity");
        assert!(activity.get("timestamps").is_none(), "{activity}");
    }

    #[test]
    fn long_lines_are_shortened_to_what_discord_accepts() {
        let mut track = playing();
        track.title = "x".repeat(400);
        let activity = activity(&track, 1_700_000_000).expect("activity");
        let details = activity["details"].as_str().unwrap();
        assert_eq!(details.chars().count(), LINE_LIMIT);
        assert!(details.ends_with('…'));
    }

    #[cfg(unix)]
    #[test]
    fn the_worker_handshakes_publishes_and_clears() {
        use std::io::Read;
        use std::os::unix::net::UnixListener;

        fn read_frame(stream: &mut std::os::unix::net::UnixStream) -> (u32, Value) {
            let mut header = [0u8; 8];
            stream.read_exact(&mut header).expect("frame header");
            let opcode = u32::from_le_bytes(header[0..4].try_into().unwrap());
            let length = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
            let mut body = vec![0u8; length];
            stream.read_exact(&mut body).expect("frame body");
            (opcode, serde_json::from_slice(&body).expect("frame json"))
        }

        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("discord-ipc-0");
        let listener = UnixListener::bind(&path).expect("listener");
        let (tx, rx) = mpsc::channel();
        let connected = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&connected);
        let worker = std::thread::spawn(move || run(rx, vec![path], flag));

        tx.send(Command::Connect("1234".to_string())).unwrap();
        let (mut stream, _) = listener.accept().expect("connection");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();

        let (opcode, handshake) = read_frame(&mut stream);
        assert_eq!(opcode, OP_HANDSHAKE);
        assert_eq!(handshake["client_id"], "1234");
        assert_eq!(handshake["v"], 1);

        tx.send(Command::Publish(Some(playing()))).unwrap();
        let (opcode, published) = read_frame(&mut stream);
        assert_eq!(opcode, OP_FRAME);
        assert_eq!(published["cmd"], "SET_ACTIVITY");
        assert_eq!(published["args"]["pid"], std::process::id());
        assert_eq!(published["args"]["activity"]["state"], PRE_RELEASE_LABEL);

        // Pausing takes the presence away instead of freezing it.
        let mut paused = playing();
        paused.playing = false;
        tx.send(Command::Publish(Some(paused))).unwrap();
        let (_, cleared) = read_frame(&mut stream);
        assert_eq!(cleared["cmd"], "SET_ACTIVITY");
        assert!(cleared["args"]["activity"].is_null(), "{cleared}");

        tx.send(Command::Shutdown).unwrap();
        worker.join().expect("worker");
        assert!(!connected.load(Ordering::Relaxed));
    }
}
