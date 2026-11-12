linux development is currently stalled cuz i currently dont have anybody whos willing to test on it, but it will be resumed once sonora gets to a more usable state

> [!WARNING]
> this is ai slop
> i am just an unemployed retard whose expertise does not include coding
> 
> i make slight adjustments and will continue until this project fully meets my expectations whilst also probably going to be usable for other h*man beings, as i dont consider my current ideas to be opinionated

> [!CAUTION]
> sonora is still an early work in progress - currently not suitable for casual or everyday usage, and some of the features described below are incomplete/yet to be made
>
> [track the status](#status) [or commit history](/commits/main/)

<img width="1920" height="1080" src="https://github.com/user-attachments/assets/77d3a20e-67e6-4013-956e-da768ecfd641" />

# this is *sonora,* your new home for all things music.

an attempt at bringing your entire music experience together in one place without having to juggle a billion different applications to do so.

**platforms:** winjews & loonix + android (planned)

**built with:** rust · slint · gstreamer · sqlite

**no jewscript or electroon.**

---

## ok but wdym bro

the idea is pretty simple: one application for managing, listening to, discovering and eventually acquiring your music.

rather than being just another music player, sonora is intended to be a complete music library and listening companion.

### the plan

- **your music, your library** — scan, organise, browse and play your local collection, with proper metadata, artwork, history and playlists.
- **online stuff** — discover music, retrieve metadata and artwork, look up lyrics and connect your listening activity to last.fm.
- **soulseek integration** — search for and download music directly through soulseek, with proper transfer management and library integration.
- **device synchronisation** — keep external music players in sync with your desktop library, including compatible artwork and folder layouts.
- **one experience** — a consistent interface across windows, linux and android, with platform-specific features where they make sense.

the point isn't to replace every music service out there. it's to give your own music collection an actual home.

## status

sonora is under active development. the project is being built in phases, and not everything described in the plan is available yet.

### what works right now

**library & organisation**
- Recursive library scanning for MP3, FLAC, M4A/AAC, OGG, Opus, WAV, AIFF, WavPack, Monkey's Audio, Musepack and WMA.
- Metadata extraction through `lofty`, with filename fallback when tags are missing.
- Tag editing from a track's menu: title, artist, album, album artist, year, track and disc are written into the file itself and read back, so only what the format actually stored reaches the library.
- Embedded artwork writing, plus `cover.jpg`/`cover.png` export that never replaces an existing cover without approval.
- Incremental rescanning: unchanged files are skipped and deleted files are pruned.
- SQLite library with FTS5 search, album/artist aggregation and library statistics.
- Album and artist pages, including album track ordering, artist-wide track browsing and a picture on both profiles.

**playback & queue**
- GStreamer-based playback with play/pause, previous/next, seeking and volume control.
- Gapless playback handover.
- Persistent playback queue that survives application restarts.
- Apple Music-style queue behaviour:
  - Shuffle upcoming tracks without rearranging the currently playing track or history.
  - Play next and add to queue.
  - Drag-and-drop queue reordering.
  - Repeat off / all / one.
- Playback history, play and skip counts, ratings and favourites.
- Most played shelf on Home.
- ReplayGain support, with track gain taking priority over album gain.
- Audio output selection, saved for the next restart to keep playback stable.
- MPRIS media controls on Linux: media keys, lock screen and panel widgets read and drive the transport.

**artwork, metadata & lyrics**
- Embedded artwork extraction and conversion.
- Configurable artwork format, quality and maximum dimensions.
- Content-addressed artwork storage.
- Artwork in track rows, album cards, the transport bar and both profile headers, decoded off the UI thread through one bounded cache.
- Artist photos from Deezer's keyless search, kept in the library so a profile opens offline.
- Online metadata providers through MusicBrainz and other sources.
- Cover artwork retrieval with multiple provider fallbacks.
- Lyrics retrieval through LRCLIB, with cached lyrics available offline.
- Automatic lyrics loading, manual scrolling and synced-line following.
- Hand-synced lyrics: a Sync mode stamps each line at the playhead, undoes a stamp and saves LRC for lines that were actually timed.

**last.fm & discord**
- Last.fm scrobbling: a play counts under Last.fm's listening rules (half the track or four minutes, never under thirty seconds), is queued in the library and sent when the network allows, and the sign-in is a single browser approval.
- Discord Rich Presence on desktop, showing the track and `sonora pre-release` while something is playing and hiding itself on pause. Off by default.

**interface & settings**
- Slint desktop interface with Discover/Home, Tracks, Playlists, Albums, Artists and Settings.
- Favourites live under Playlists, History opens from Home, and library folders live in Settings.
- Global library search.
- First-run onboarding, replayable from Settings.
- Keyboard shortcuts for playback, seeking, volume, search and the lyrics page.
- Failures shown as a card that says what went wrong, instead of a disappearing status line.
- Accessible names and roles on every control, and landmark structure for the page.
- Panels that move instead of appearing, and rows, cards and buttons that ease between states.
- Format preference and bitrate ceiling settings.
- Cache budget, confirmed cache clearing and artwork configuration.
- Configurable history retention (7 days by default).
- Configurable library folder template.
- Last.fm key pair and connection, and the Discord application id.
- Download and cache folder pickers.
- Device-sync opt-in setting.
- Queue and Lyrics share one independently scrollable side panel.

### what's still yet to come

the following are either partially implemented or waiting for their respective development phase:

- Last.fm beyond scrobbling: loved tracks, charts, similar artists and recommendations.
- Discord Rich Presence on Android, through the Discord Social SDK.
- SMTC media controls on Windows, and the Android media session.
- The artwork candidate review screen before a change is applied.
- External device detection, sync profiles and resumable transfers.
- Soulseek search, downloads, transfer management and library importing.
- Android build infrastructure, touch-first interface and background playback.
- Packaging (Windows installer, AppImage/Flatpak, signed APK), GPU and slower-disk performance measurement, and the remaining format gaps (AAC, ALAC, APE, Musepack and AIFF are unverified here; WMA needs a plugin set this project does not ship).

### platform support

winjews & loonix: desktop build targets
android: first-class planned target; build harness and touch interface still in development

currently the desktop platforms are the only ones producing runnable application binaries

android is being developed alongside the desktop phases, not treated as an afterthought. its implementation has additional requirements around storage permissions, background playback, media sessions and touch interaction. also the library sync feature is intentionally left out

for the actual development phases and acceptance criteria, see the [roadmap](docs/ROADMAP.md).

## docs

- [ARCHITECTURE](docs/ARCHITECTURE.md) — crate layout, threading, database schema and internal structure
- [DEVELOPMENT](docs/DEVELOPMENT.md) — setup, conventions, tooling and testing
- [ROADMAP](docs/ROADMAP.md) — development phases, current progress and acceptance criteria
- [REQUIREMENTS](docs/REQUIREMENTS.md) — detailed product requirements for upcoming features
- [LICENSING](docs/LICENSING.md) — dependency licence audit and related decisions

## how 2 build

for now, these commands build the desktop target

android has its own NDK build requirements, which are documented in [DEVELOPMENT.md](docs/DEVELOPMENT.md), but the harness isn't wired into the repository yet

**requirements**

- rust 1.96+ ([rustup](https://rustup.rs))
- for linux: a C toolchain, `pkg-config` and the required GStreamer development packages.
- for windows: a compatible Rust toolchain and the required GStreamer installation.

**nixos**

```sh
nix-shell
cargo build --workspace
```

this exposes the real fontconfig and gstreamer development libraries. see [DEVELOPMENT](docs/DEVELOPMENT.md) for details.

**build & test**

```sh
cargo build --workspace
cargo test --workspace
```

**run the desktop application**

```sh
cargo run -p sonora-app
```

**scan a folder without opening the window**

handy for testing and debugging library indexing.

```sh
cargo run -p sonora-app -- --scan ~/Music
```

the library database lives in the platform-specific application data directory.

```sh
sonora --where
```

you can override the location with `SONORA_DATA_DIR`.

for additional setup details, platform-specific requirements and development conventions, consult [DEVELOPMENT.md](docs/DEVELOPMENT.md).

## license

sonora is free software licensed under **GPL-3.0-or-later**.

## credit

thanks:
- [taskynn](https://www.instagram.com/tas.kynn/) for turning my design mockup into a figma project
- [moxiu](https://github.com/cfels/) for porting the aforementioned project & adding a nix flake
