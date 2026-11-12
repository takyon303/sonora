//! End-to-end check against a real (private) session bus.
//!
//! The test starts its own `dbus-daemon`, points the process at it, publishes
//! the player and then talks to it the way `playerctl` would. It is skipped with
//! a note when the machine has no `dbus-daemon`, because a missing bus is a
//! property of the environment, not of the player.

#![cfg(target_os = "linux")]

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command as Process, Stdio};
use std::time::{Duration, Instant};

use sonora_mpris::{
    Command, LoopStatus, Mpris, NowPlaying, PlaybackStatus, PlayerState, BUS_NAME, OBJECT_PATH,
};

/// A private session bus that lives exactly as long as the test.
struct PrivateBus {
    process: Child,
    _directory: tempfile::TempDir,
}

impl PrivateBus {
    /// `None` when this machine cannot start a session bus.
    fn start() -> Option<Self> {
        let directory = tempfile::tempdir().ok()?;
        let socket = directory.path().join("bus");
        let mut process = Process::new("dbus-daemon")
            .arg("--session")
            .arg("--nofork")
            .arg("--print-address=1")
            .arg(format!("--address=unix:path={}", socket.display()))
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;

        let stdout = process.stdout.take()?;
        let mut reader = BufReader::new(stdout);
        let mut address = String::new();
        reader.read_line(&mut address).ok()?;
        let address = address.trim();
        if !address.starts_with("unix:path=") {
            return None;
        }

        std::env::set_var("DBUS_SESSION_BUS_ADDRESS", address);
        Some(Self {
            process,
            _directory: directory,
        })
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

fn snapshot() -> PlayerState {
    PlayerState {
        status: PlaybackStatus::Paused,
        track: Some(NowPlaying {
            track_id: 12,
            title: "Says".to_string(),
            artist: "Nils Frahm".to_string(),
            album: "Spaces".to_string(),
            length_ms: 204_000,
        }),
        position_ms: 1_500,
        volume: 0.5,
        can_go_next: true,
        can_go_previous: false,
        can_seek: true,
        shuffle: false,
        repeat: LoopStatus::Playlist,
    }
}

fn wait_for_status(
    player: &zbus::blocking::Proxy<'_>,
    expected: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let status: String = player.get_property("PlaybackStatus")?;
        if status == expected {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("status stayed {status:?}, expected {expected:?}").into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_shell_can_see_the_player_and_drive_it() -> Result<(), Box<dyn std::error::Error>> {
    let Some(_bus) = PrivateBus::start() else {
        eprintln!("skipping MPRIS bus test: this machine cannot start dbus-daemon");
        return Ok(());
    };

    let mpris = Mpris::start(snapshot()).expect("the private bus should accept the player");
    let connection = zbus::blocking::Connection::session()?;

    let root =
        zbus::blocking::Proxy::new(&connection, BUS_NAME, OBJECT_PATH, "org.mpris.MediaPlayer2")?;
    assert_eq!(root.get_property::<String>("Identity")?, "Sonora");
    assert_eq!(root.get_property::<String>("DesktopEntry")?, "sonora");
    assert!(!root.get_property::<bool>("CanQuit")?);
    assert!(root.get_property::<bool>("CanRaise")?);

    let player = zbus::blocking::Proxy::new(
        &connection,
        BUS_NAME,
        OBJECT_PATH,
        "org.mpris.MediaPlayer2.Player",
    )?;

    assert_eq!(player.get_property::<String>("PlaybackStatus")?, "Paused");
    assert_eq!(player.get_property::<String>("LoopStatus")?, "Playlist");
    assert_eq!(player.get_property::<i64>("Position")?, 1_500_000);
    assert!(player.get_property::<bool>("CanGoNext")?);
    assert!(!player.get_property::<bool>("CanGoPrevious")?);
    assert!(player.get_property::<bool>("CanSeek")?);
    assert!((player.get_property::<f64>("Volume")? - 0.5).abs() < f64::EPSILON);

    let metadata: HashMap<String, zbus::zvariant::OwnedValue> = player.get_property("Metadata")?;
    for key in ["mpris:trackid", "mpris:length", "xesam:title"] {
        assert!(metadata.contains_key(key), "metadata is missing {key}");
    }

    // A media key reaches the UI as a command.
    player.call_method("PlayPause", &())?;
    assert_eq!(mpris.take_commands(), vec![Command::PlayPause]);
    player.call_method("Next", &())?;
    assert_eq!(mpris.take_commands(), vec![Command::Next]);

    // A seek is relative and arrives in microseconds.
    player.call_method("Seek", &(-1_000_000_i64))?;
    assert_eq!(mpris.take_commands(), vec![Command::SeekBy(-1_000_000)]);

    // Writable properties become commands too.
    player.set_property("Volume", 0.8_f64)?;
    assert_eq!(mpris.take_commands(), vec![Command::SetVolume(0.8)]);
    player.set_property("Shuffle", true)?;
    assert_eq!(mpris.take_commands(), vec![Command::SetShuffle(true)]);

    // A new snapshot reaches the shell.
    mpris.update(PlayerState {
        status: PlaybackStatus::Playing,
        ..snapshot()
    });
    wait_for_status(&player, "Playing")?;

    // SetPosition for another track is ignored, per the specification.
    mpris.update(snapshot());
    let foreign = zbus::zvariant::ObjectPath::try_from("/org/sonora/track/99")?;
    player.call_method("SetPosition", &(foreign.clone(), 5_000_000_i64))?;
    assert!(mpris.take_commands().is_empty());
    let current = zbus::zvariant::ObjectPath::try_from("/org/sonora/track/12")?;
    player.call_method("SetPosition", &(current, 5_000_000_i64))?;
    assert_eq!(mpris.take_commands(), vec![Command::SeekTo(5_000_000)]);

    Ok(())
}
