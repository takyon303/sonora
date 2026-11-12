# Roadmap

Each phase ends with something a person can actually use. A phase is only done
when its acceptance criteria pass.

Product requirements collected for later phases (Soulseek identity modes,
sharing, keyless metadata and artwork, queue rules, format preference, cache
eviction, device sync, folder layout, onboarding) live in
[REQUIREMENTS.md](REQUIREMENTS.md). That document is the source of truth for
what each phase has to cover.

## Phase 1 — Foundation ✅

Delivered:

- Cargo workspace with one-way crate dependencies.
- SQLite schema, migrations, repositories, FTS5 search.
- Recursive incremental scanner with tag extraction and file-name fallback.
- Slint shell: sidebar navigation, Home, Tracks, Albums, Library folders,
  global search, live scan status. Folder management now lives in Settings.
- CLI: `--scan`, `--where`, `--version`.
- Unit and integration tests for parsing, database behaviour and scanning.

Acceptance: scanning a folder twice imports then reports everything unchanged;
deleting a file removes it from the library on the next scan. ✅

## Phase 2 — Playback and library (in progress)

Delivered:

- `sonora-playback`: GStreamer engine over `playbin` (not `playbin3`, which has
  no `about-to-finish`), gapless handover, play/pause/stop, previous/next, seek,
  volume, the full queue API and a caller-supplied track resolver.
- Apple-Music-style queue rules (shuffle only upcoming, play next, add to queue,
  drag reorder, repeat off/all/one), tested in `sonora-core::queue`.
- Transport bar (track, play/pause, previous/next, continuous scrubbing, volume),
  per-row covers and play/Next/Add actions and a drag-reorderable queue panel.
  A serial worker handles track switches without blocking input; cover changes
  fade over 140 ms. Appearance switches persist panel blur and cover backgrounds.
- Queue persistence: the queue snapshot survives a restart, including shuffle
  state and repeat mode.
- Settings: format preference and ceiling, cache budget, artwork format/quality,
  folder template, the Last.fm key, native download/cache folder pickers and the
  device-sync opt-in. History retention defaults to seven rolling UTC days;
  saving a shorter period removes expired events while preserving lifetime
  counts, ratings and favourites. Track-cache clearing asks for confirmation
  and protects queued files and library originals.
- Play activity: history, play and skip counts, star ratings and favourites,
  written by the UI, shown on Home (Most played), History and Favourites, and
  readable headlessly with `sonora --history`.
- ReplayGain from tags: the scanner reads track and album gain and peak
  values, the database persists them, and the engine applies the track gain
  (album gain as the fallback) as a linear volume factor so loudness is even
  across files. Tracks carrying a gain show it as a badge in the list.
- Output device selection: the Settings screen opens a picker listing the
  GStreamer output backends this system has; the choice is stored in settings
  and applied on the next launch. The engine refuses to replace a live sink.
- Album pages: clicking a release in Albums opens its tracks in disc/track
  order (untagged files last), with one action that plays the whole album
  and the usual per-row play, queue, rating and favourite controls.
- Artist pages: the artist credit on an album opens everything by that
  artist — their albums and every track they perform — with one action that
  plays them all. Back returns to the release the artist was opened from.
- Custom playlists: name one, add the track you are playing from any row's +
  action, play the list, rename it, take tracks out, and delete it — the
  tracks themselves stay in the library.
- Smart playlists: a playlist whose tracks are selected by rules (a text
  filter, favourites only, a star floor and an earliest year) rather than by
  hand. The rules are stored as a document, the tracks are derived on every
  read, so rating a track, favouriting it or removing a file changes the list
  without editing it. Hand edits — adding, removing, reordering — are refused
  on a smart playlist, and its page edits the rules instead.
- Artwork instead of placeholders. Track rows show the file's embedded art, and
  album cards borrow the art of the release's first track in disc order; both
  decode off the UI thread through one bounded cache and only request art for
  what is on screen. A release whose files are gone keeps its card, without a
  cover.
- MPRIS media controls on Linux. The window publishes
  `org.mpris.MediaPlayer2.sonora` on the session bus with both halves of the
  protocol, so media keys, lock-screen controls and panel widgets work. It
  reports playback status, track metadata (title, artist, album, length and a
  track id), position, volume, shuffle, loop status and the can-go flags, and
  turns `PlayPause`, `Play`, `Pause`, `Stop`, `Next`, `Previous`, `Seek`,
  `SetPosition`, `SetVolume`, `SetShuffle`, `SetLoopStatus` and `Raise` back
  into engine calls. The bus runs on its own thread as a snapshot plus a
  command queue, so a missing or wedged shell never stalls the window, and
  properties are signalled only when they change. A test publishes the player
  on a private `dbus-daemon` and drives it the way a shell would.

Still open:

- SMTC (Windows) media controls; Android uses its platform media session
  instead (see the Android track).

Acceptance: playback survives restart with its queue; play counts and history
are written per track; a smart playlist updates live as rules change.

## Phase 3 — Artwork (in progress)

Delivered:

- Embedded artwork extraction, JPEG/PNG conversion with configurable quality and
  max dimension, alpha flattened onto white, `cover.jpg`/`cover.png` export that
  never replaces an existing cover without approval, blake3 content-addressed
  store, batch report.

- Tag editing: a track's menu opens a form with title, artist, album, album
  artist, year, track and disc. Saving writes those tags into the file itself,
  reads it back and re-indexes the row, so only what the format actually stored
  reaches the library; an emptied field removes that tag, and the file is never
  renamed or copied. Ratings, play counts and favourites survive an edit.
- Embedded artwork writing: `sonora --embed-cover <audio> <image>` writes an
  image into a file's primary tag (ID3v2, Vorbis comments, MP4 ilst, APE, AIFF
  text) as its front cover, replacing any cover already there, and reads it
  back to confirm. The primitive is `sonora_artwork::embed_cover`, exercised by
  tests for WAV.

Still open:

- Candidate review UI and Rockbox layout.

Acceptance: a 2 000-album library converts without corrupting any tag, and the
review screen shows every proposed change before it is applied.

## Phase 4 — Online (in progress)

Delivered:

- `sonora-metadata`: MusicBrainz → Cover Art Archive, Deezer and iTunes artwork
  fallbacks, LRCLIB synced and plain lyrics, Last.fm tags behind an API key, TTL
  disk cache, every provider behind an `Http` trait so tests never touch the
  network.
- Lyrics stored per track in the `lyrics` table, and reachable headlessly with
  `sonora --lyrics <query>` and `sonora --cover <query>`.
- A lyrics pane in the player: it shows saved lyrics first (so a second open is
  offline and instant), fetches automatically when opened or when the track
  changes, and keeps the result. Local `.lrc` files take precedence. In-flight
  and recent failed/empty requests are deduplicated; stale results are ignored.
- Cover art in the transport bar, decoded off the UI thread from the file's
  embedded picture and refreshed whenever playback moves to another track.
- Synced lyrics follow playback line by line: the pane splits LRC into timed
  lines, highlights the current one from the transport clock and eases it into
  view, for LRCLIB results and local `.lrc` files alike. The pane is its own
  page over an opaque backdrop, so the library view behind it never shows
  through. Manual scrolling suspends following; the Follow playback control
  appears only while following is off, and the queue rail can be opened beside
  the page instead of replacing it.

Still open:

- Artist and album profiles with pictures.
- Last.fm (needs a free developer API key): auth, scrobbling with an offline
  queue, now-playing, loved tracks, charts, similar artists, recommendations,
  cached responses.
- Lyric editing: hand-syncing a plain lyric, or fixing a bad LRC.
- Discord Rich Presence (hidden while paused, fully disableable): RPC/IPC on
  desktop, and on Android through the Discord Social SDK, which publishes
  presence without account linking when the Discord app is installed and signed
  in (SDK 1.10+, Android 7.0+).
- Discovery homepage that degrades to local data when offline.

Acceptance: an offline session's scrobbles replay correctly on reconnect; the
homepage renders with the network unplugged.

## Phase 5 — Devices (Windows and Linux)

- Volume detection: Linux (mount table + udev) and Windows (SetupAPI). Android
  has no device sync at all; see the Android track.
- Sync profiles, diff planning against a device manifest, resumable copies with
  progress, free-space checks, safe unplug handling.
- Rockbox-compatible artwork export. The Y1 is treated as plain USB mass
  storage; Rockbox is an option, never an assumption.

Acceptance: unplugging mid-transfer and re-plugging resumes without duplicates.

## Phase 6 — Downloads

- Spike: `soulseek-rs-lib` versus an external `slskd` daemon, judged on licence,
  stability and packaging cost. Backend hidden behind a trait either way.
- Identity modes: sign in with a real account, or opt into a rotated random
  identity with its rate-limit and ToS caveats surfaced in the UI.
- Sharing gate: shared-folder uploads, or a clear explanation when a peer
  refuses downloads from non-sharing users.
- Search with filters, transfer queue, history, import with metadata and art
  into the user's chosen download folder, laid out by the folder template.
- Legal acknowledgement before first use.

## Android track (parallel from phase 2)

Android is a first-class target, so its work runs alongside the desktop phases
instead of after them.

- NDK build harness (`cargo-apk` or a Gradle + `cargo-ndk` project) producing an
  APK that links the Slint Android backend and a GStreamer Android build of the
  same plugin set, for arm64-v8a plus an emulator ABI.
- Responsive, touch-first layout: bottom navigation, single-column lists, a
  queue sheet, larger hit targets, no hover-only actions.
- Library access through the Storage Access Framework, with the same incremental
  scan and FTS search as desktop.
- Playback in a foreground service with a media notification and lock-screen
  controls, surviving backgrounding and screen-off.
- Downloads and cache inside app-scoped storage, with an optional user-granted
  folder; no external device sync (a phone is a playback client, never a sync
  host).
- Discord Rich Presence through the Discord Social SDK, published without account
  linking when the Discord app is installed; system media controls through
  `MediaSession` and the media notification.

Acceptance: on a physical Android device, pick a folder in the system picker,
scan it, play a track gaplessly with the screen off, and get the same queue back
after the app is killed and reopened.

## Phase 7 — Polish (in progress)

Delivered:

- Native shell geometry and vector assets taken from the supplied Figma file;
  the repair report records the remaining visual limits. Favourites are under
  Playlists and History opens from Home.
- First-run onboarding: welcome, music folders, format preference and the
  optional Last.fm key, skippable at every step, plus **Run onboarding again** in
  settings. The Soulseek sharing notice and device setup join the flow when
  those features exist, rather than being shown before they mean anything.
- Motion: panels move instead of appearing. The queue rail slides past the
  window edge, the track menu and both dialogs fade and lift into place, and
  rows, cards, buttons and menu entries ease between their resting and hovered
  colours. The queue rail is the only one driven by a state transition, so
  resizing the window still moves it immediately.
- Keyboard shortcuts: Space plays or pauses, Ctrl+Left/Right skip, Ctrl+Shift+
  Left/Right seek ten seconds, Ctrl+Up/Down move the volume, Ctrl+F jumps to
  search, Ctrl+L opens the lyrics page and Escape closes the menu, dialog or
  lyrics page. A focus scope around the window captures them, so they work
  wherever focus is, without stealing a space or an escape from a field being
  typed in; the Settings page lists them.
- Error reporting: a failure is a card, not a status line. It wears the only
  warm colour in the shell, stays on screen even with the Status messages
  switch off, wraps instead of truncating the reason, goes away when clicked,
  and is replaced by the next message. Scanning, folders, settings, playback,
  tag saving, the cache, artwork and playlists all report through it, so a scan
  that died can no longer look like one still running.
- Accessibility: every control reports a role and a name, so a screen reader
  hears "Shuffle: on" instead of "button"; rows, cards, menu entries and the
  queue read as one item — a track is "Opening, Nils Frahm, 3:24" — and offer
  a default action that plays or opens what they stand for. The sidebar, the
  section list, the search field, the page area and the status card are
  landmarks, so the window has a structure to move around in. A test walks the
  compiled element tree and fails on any control that has no name; debug builds
  carry that tree, release builds do not.

Still open:

- Packaging: Windows installer, Linux AppImage/Flatpak, signed Android APK/AAB.
- Performance work beyond what is measured below: frame timing on a real GPU
  backend, and a library on a slower disk.
- Format coverage, now measured against a real corpus rather than asserted:
  with the `base` and `good` plugin sets FLAC, MP3, Vorbis, Opus, WAV, WavPack
  and Speex decode end to end through `playbin` (a 2,000-file scan indexes
  them in under half a second), and the scanner reports the error for a file it
  cannot read instead of dropping it silently. Still unverified here: AAC,
  ALAC, APE and Musepack need `bad` or `libav`, AIFF has no parser in this
  plugin set, and WMA needs the ASF demuxer from `ugly`, which this project
  does not ship. The decoder table in `sonora-core` now says which plugin set
  each container needs instead of claiming they all decode.
- Headerless VBR MP3s can report a wildly wrong duration: a 2.04 second file
  was read as 379 ms, because the estimate trusts the first frame's bitrate.
  The scanner uses `lofty`'s estimate today; a better fix is to take the
  duration from GStreamer discovery, which is a bigger change and is deferred.

Measured on a synthetic library of 2,000 files, release build, at 1920×1080
through the software renderer:

- A cold scan of the whole library takes 419 ms; a rescan with nothing changed
  takes 187 ms.
- A redraw of the Tracks page with 2,000 rows costs 8.5 ms and the Settings page
  12.3 ms. The track list only lays out the rows on screen, so its cost does not
  grow with the library.

Nothing in those numbers asks for a change yet. The software renderer is the
pessimistic case, so a GPU backend should be faster rather than slower.
