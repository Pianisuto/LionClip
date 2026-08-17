# LionClip Architecture

## Goals

LionClip is a resident Linux desktop utility whose primary interaction is a small clipboard-history popup invoked through a global desktop shortcut.

The architecture optimizes for:

- low idle overhead;
- fast popup latency;
- local-only data;
- GNOME/Zorin visual integration;
- explicit isolation of platform-specific behavior;
- easy iteration by small PRs.

It does not optimize for extensibility into a general automation platform.

## Process model

V1 should prefer a **single resident process**.

```text
                       LionClip process
                              |
          +-------------------+-------------------+
          |                   |                   |
          v                   v                   v
   Clipboard service      History service       Popup UI
          |                   |                   |
          |                   v                   v
          |                SQLite             GTK4/Adw
          |                                       |
          +-------------------+-------------------+
                              |
                              v
                     Positioning backend
                       |             |
                X11 (validated)   fallback
```

A second helper process, GNOME Shell extension, daemon, or privileged component must not be introduced unless a measured technical limitation requires it and the decision is documented.

## Module responsibilities

The exact file tree may evolve, but ownership should remain close to this model:

```text
src/
├── main.rs
├── app/
│   ├── lifecycle.rs
│   └── commands.rs
├── clipboard/
│   ├── monitor.rs
│   ├── reader.rs
│   └── writer.rs
├── history/
│   ├── model.rs
│   ├── service.rs
│   ├── repository.rs
│   └── dedup.rs
├── popup/
│   ├── window.rs
│   ├── item.rs
│   ├── search.rs
│   └── keyboard.rs
├── positioning/
│   ├── mod.rs
│   ├── x11.rs
│   └── fallback.rs
├── paste/
│   ├── mod.rs
│   └── x11.rs
├── settings/
│   ├── mod.rs
│   ├── schema.rs
│   ├── service.rs
│   ├── autostart.rs
│   └── window.rs
└── storage/
    ├── db.rs
    └── migrations.rs
```

Do not create all of these files before their responsibilities exist. The tree is a direction, not scaffolding homework.

## Application lifecycle

### Application ID

The final application ID is `io.github.Pianisuto.LionClip`. It is the D-Bus
name GIO uses for single-instance behavior, and the base name of every
installed desktop-integration file:

```text
/usr/share/applications/io.github.Pianisuto.LionClip.desktop
/etc/xdg/autostart/io.github.Pianisuto.LionClip.desktop
/usr/share/metainfo/io.github.Pianisuto.LionClip.metainfo.xml
/usr/share/icons/hicolor/*/apps/io.github.Pianisuto.LionClip.{svg,png}
```

It follows the GNOME reverse-DNS convention for a project hosted at
`github.com/Pianisuto/LionClip` and keeps the GitHub account and repository
spelling, so the ID reads as the project it belongs to. AppStream flags the
capital letters as a pedantic note, not an error, and GNOME applications
commonly carry them. A unit test asserts that the constant in `cli.rs` and the
packaged files still agree, because a divergence here breaks activation,
the launcher icon and single-instance behavior at once, and only on an
installed system.

### Commands

```text
lionclip          start the resident instance, leave the popup alone
lionclip show     show the popup
lionclip hide     hide the popup, keep the process resident
lionclip toggle   show the popup when hidden, hide it when visible
lionclip --help | --version
```

`cli::parse` is pure: it turns arguments into a `Command` or into an `Answer`
the invoked process prints itself. `--help`, `--version` and invalid arguments
are answered before any application object exists, so they never open a
display, never register on the bus and never start a monitor. Invalid arguments
print one line plus the usage line and exit with status 2.

### Command routing and single instance

The application runs with `HANDLES_COMMAND_LINE`. The first process to own the
application ID becomes the resident instance and builds the whole application
state — history, clipboard monitor, popup — once, in `startup`. Every later
invocation finds the name taken, hands its argument vector to that instance
over D-Bus and exits with the code the instance returns. Repeated `Super+V`
presses are therefore commands to one process, and there is only ever one
clipboard monitor.

`activate` is deliberately left unconnected. The command line is the only entry
point, which is what guarantees the autostart invocation cannot open the popup
at login.

The resident instance stays alive on an `ApplicationHoldGuard` taken when the
state is built. An instance that fails to build state takes no guard, so it
exits instead of lingering as a broken primary that later invocations would
reach.

### Toggle

`Command::intent(popup_visible)` is the single decision point, and it is a pure
function with tests:

```text
Run    -> Leave
Show   -> Show
Hide   -> Hide
Toggle -> Hide when the popup is visible, Show when it is not
```

A visible popup is hidden, never hidden and shown again: the show path places
the window before the compositor maps it, so re-running it on a window that is
already on screen would move a popup the user is looking at. `Show` on an
already visible popup only returns the focus to the search field, for the same
reason.

## Clipboard service

Preferred behavior:

1. subscribe to clipboard-change notifications using GDK/GTK APIs where they provide the needed behavior;
2. inspect advertised formats;
3. asynchronously read supported content;
4. normalize it into a domain item;
5. send it to the history service;
6. never print payload content to logs.

Avoid polling unless the target compositor/session demonstrably requires a narrowly-scoped fallback.

### Supported content roadmap

Initial:

- UTF-8 text;
- multiline text;
- URLs as text;
- source code as text.

Later:

- images/screenshots;
- selected additional MIME types only when there is a clear UX.

## History model

A future persisted item should conceptually include:

```text
id
kind
mime_type
text_content or blob reference
content_hash
created_at
last_used_at
pinned
```

Exact columns belong to the persistence phase, not the technical spike.

### Deduplication

Repeated copies of the same logical content should not produce a noisy stack of identical rows. Re-copying existing content should move/update that logical entry according to the current history ordering rules.

Hashing is an implementation detail and must be defined consistently for each content kind.

### Ordering

`TextHistory` keeps one deterministic order:

1. pinned items first, then unpinned items;
2. inside each group, `last_used_sequence` descending.

Pinning moves an item into the pinned group without changing its own recency;
re-copying an item refreshes its recency inside its group. Restoring an item to
the clipboard deliberately does not change ordering, because the restore is
suppressed as a self-write and never re-enters history.

### History operations

The UI never mutates a `TextHistoryItem` and never sees a SQLite connection. It
calls explicit domain operations on `TextHistory`:

```text
record(text)        -> Inserted | MovedToFront | Unchanged
pin(id) / unpin(id) -> Applied | Rejected
delete(id)          -> Applied | Rejected
clear_unpinned()    -> Applied | Rejected
search(&query)      -> Vec<&TextHistoryItem>
```

Operations on unknown identifiers, or that would not change anything, return
`Rejected` and submit no persistence mutation. Every applied operation updates
the in-memory source of truth first and then queues the matching mutation on the
database worker.

### Search

`HistoryQuery` is a small filter layer over the already-loaded items:
case-insensitive substring matching, trimmed query text, original content
preserved, input order preserved. The popup re-runs it on every keystroke
against memory; search never queries SQLite and query text is never logged.

## Storage

Text history is persisted in SQLite at
`$XDG_DATA_HOME/lionclip/lionclip.db`, with the standard
`$HOME/.local/share` fallback when `XDG_DATA_HOME` is unset. Schema changes use
ordered migrations recorded in SQLite's `user_version`; schema version 1 has a
single `history_items` table:

```text
id                    stable INTEGER PRIMARY KEY
kind                  "text" in schema v1
text_content          exact, lossless clipboard text
created_sequence      monotonic logical creation order
last_used_sequence    monotonic logical recency order
pinned                retention exemption (UI arrives in Phase 3)
```

The logical sequences avoid wall-clock ties and make restart ordering
deterministic. Schema v1 already expresses pin, delete and clear, so Phase 3
introduced no schema v2; the UI intentionally shows no timestamps rather than
migrating the schema for decoration.

`TextHistory` owns insertion, exact-content deduplication, move-to-front
behavior, identity allocation, pin state, deletion, and the 500-unpinned-item
retention policy. A single dedicated worker applies the resulting mutations
(`Upsert`, `Delete`, `ClearUnpinned`) to SQLite in channel order, keeping
synchronous database work out of clipboard and GTK rendering callbacks. Clearing
is one statement in one transaction rather than one command per removed item. Normal worker shutdown drains accepted commands before the
database connection closes. SQLite uses a five-second busy timeout and enables
foreign-key enforcement for migration safety; WAL is intentionally not enabled
because LionClip has one serialized database path and does not need concurrent
readers.

The migration runner walks a small ordered migration table. Each pending
version runs in its own transaction and advances `user_version` only after its
schema changes succeed. Databases newer than LionClip supports are rejected
without modification.

On Unix, SIGTERM and SIGINT are observed through GLib Unix signal sources on
the main context. Their callbacks request `GApplication::quit`; the normal
application shutdown signal then drops application state and drains and joins
the database worker. Popup close and Escape continue to hide the resident
process without shutting it down.

If the data path, database, or migration cannot initialize, LionClip reports a
payload-free diagnostic and runs with bounded in-memory history for that
session. A later write failure is also reported without clipboard contents;
the in-memory session remains usable.

For later content phases:

- use SQLite;
- use standard XDG data locations;
- keep schema changes versioned through migrations;
- prefer storing image/blob files in a controlled data directory instead of filling SQLite with large payloads unless measurements justify otherwise;
- clean orphaned blobs;
- set explicit retention limits.

Conceptual locations:

```text
$XDG_DATA_HOME/lionclip/lionclip.db
$XDG_DATA_HOME/lionclip/blobs/
$XDG_CONFIG_HOME/lionclip/
```

Respect XDG environment overrides.

## Popup UI

The popup is a transient utility surface, not the application's main window.

Direction:

- ~430 px width;
- max height ~500 px;
- search at top;
- virtual/lazy list behavior when necessary;
- no permanent toolbar/sidebar/status bar;
- minimal row actions, preferably on hover/focus;
- native system appearance through Libadwaita;
- closes on Escape and normally after restoring an item;
- sensible behavior when focus is lost.

For text rows, display a compact preview. Do not execute or interpret copied content.

### Current structure

```text
AdwApplicationWindow (430 px, undecorated, non-resizable)
└── GtkBox
    ├── GtkSearchEntry + GtkMenuButton (overflow: clear unpinned history)
    ├── GtkSeparator
    ├── GtkScrolledWindow → GtkListBox   (rows, max content height 360 px)
    └── empty/no-results message         (shown instead of the list)
```

Height follows content up to the list cap, so a short history stays small.
Rows are rebuilt from the filtered snapshot; row identity for every action is
`HistoryItemId`, never the GTK row index. Row actions are real buttons that stay
reachable by keyboard and are revealed on hover, selection or focus by a small
stylesheet with no hardcoded colors. The pin is a toggle, so the theme draws it
checked and the pinned state is visible on the row itself, next to the pinned
group's position in the list.

The rounded surface is the content box, not the toplevel: it carries Adwaita's
own `.background` style plus a corner radius and clips its children. The
toplevel draws nothing — background, border and shadow are all cleared — and
both of those matter. GTK marks the whole surface opaque whenever the window
background is opaque, and the compositor then skips blending and leaves black
behind the rounded corners; a themed window shadow survives a transparent
background and keeps painting a faint halo into the corner area, which reads as
a dim rectangle under the popup.

### Interaction

- typing anywhere filters, including while a result row holds focus;
- `Down` always advances from the current selection, so the first press from the
  search field lands on the second result — the first one is already selected on
  open — and `Up` on the first result returns to the search field;
- `Enter` restores the selection and hides the popup;
- `Escape` clears a non-empty search, otherwise hides the popup;
- `Delete` removes the selected item when focus is in the result list, so it
  still edits text while the search field has focus;
- `Right`/`Left` reach the selected row's pin and delete buttons and step back
  out of them, but only while the search field is empty: with text in it they
  stay with the caret;
- `Ctrl+F` focuses search, `Ctrl+P` pins/unpins the selection;
- clicking a row restores it; clicking a row action does not restore it.

`Enter` and `Space` are handed back to the focused widget whenever that widget
activates itself — a row action button or the overflow menu button — so the
window shortcuts never shadow the control the user tabbed to. The rule is
structural, not name-based: the search field, a result row and the list itself
leave activation to the popup, anything else keeps it.

### Focus-loss behavior

The popup hides when the toplevel loses focus, and only then: a keyboard grab
also deactivates the toplevel without the focus ever leaving it, which is what
pressing the desktop shortcut does while the popup is open. The X11 backend is
asked whether the popup still owns the keyboard focus, so a grab is ignored and
a real focus change is not. Backends that cannot answer fall back to the
toplevel's own activation state.

Invoking the popup while it is already open only returns the focus to the
search field. It is neither re-placed nor presented again, because presenting a
window that is already on screen lets the compositor lay it out afresh. Its own surfaces suppress that
while they are open, counted rather than flagged because the overflow menu hands
over to the confirmation dialog while it is still closing. Releasing the
suppression is itself deferred by one main-context turn after a display round
trip: dropping a menu's keyboard grab deactivates and reactivates the toplevel
in quick succession, and that transient deactivation must not be mistaken for
the user leaving. If the toplevel went inactive while suppressed, the hide
condition is re-checked once when the last surface closes, because no further
`is-active` notification would arrive on its own.

## Settings and preferences

`SettingsService` (`src/settings/`) is the single authority for LionClip
preferences. The popup, the clipboard/history services and the preferences
window all read and write through it; nothing else touches `gio::Settings`
or the autostart override file directly.

### Storage: GSettings, not a hand-rolled config file

Preferences persist through GSettings, schema `io.github.Pianisuto.LionClip`,
installed to `/usr/share/glib-2.0/schemas`. This needed no new dependency —
`gio` was already linked for application lifecycle and single-instance
command handling — and it gives native type/range validation at write time,
atomic dconf-backed storage, and the same `gsettings`/dconf tooling GNOME
itself uses for support and debugging. A hand-rolled XDG config file would
have needed a serialization dependency and its own corrupt-file handling for
no offsetting benefit on a project already this deep into GLib/GIO.

`src/settings/schema.rs` resolves the compiled schema in order: the schema
installed on the system (the normal case once packaged), a copy `build.rs`
compiles from `packaging/schemas/` into `$OUT_DIR` for `cargo run`/`cargo
test` before install, and finally a copy compiled on the fly into a private
temp directory as a last resort. All three resolve to the schema's own
declared `path`, so a development build and a real install read and write
the same dconf keys. If none of that produces a usable schema —
`glib-compile-schemas` missing everywhere — `SettingsService` falls back to
unpersisted in-memory defaults and logs a payload-free diagnostic, the same
degradation `TextHistory` already applies when the database path is
unavailable.

"Start at login" is deliberately **not** a GSettings key. Its effective state
is the presence of a per-user `~/.config/autostart/<app-id>.desktop` override
containing `Hidden=true`, which is exactly what GNOME's own Startup
Applications/Tweaks write and read, layered over the package's system-wide
`/etc/xdg/autostart` entry (`docs/ARCHITECTURE.md`'s Desktop integration
section below). Storing a second "enabled" boolean in GSettings could
silently disagree with what the override file actually says, so the
filesystem stays the one source of truth (`src/settings/autostart.rs`).

### Reacting to changes

Settings are read directly, at the point of use, with no caching and no
change-notification/signal-wiring layer: the clipboard capture handler calls
`settings.recording_paused()` and `settings.save_images()` on every clipboard
change, and the popup calls `settings.recording_paused()` every time it
rebuilds to decide whether to show its "History paused" indicator. In this
process, GSettings itself is the only writer these consumers care about, and
a live read is cheap, so there was nothing a signal would buy beyond
indirection. The one setting that needs a proactive reaction is `history
limit`, because nothing else naturally triggers a retention pass: the
preferences window calls `TextHistory::set_unpinned_limit` directly, which
runs the existing `enforce_retention` path immediately, the same one that
already runs after every insert.

### Preferences window

`AdwPreferencesWindow` (`src/settings/window.rs`) is a normal GNOME window,
built once and reused for the process lifetime: it hides rather than closes
on the window-close button, exactly like the popup does, so `lionclip
settings` and the popup's "Preferences" menu item both reuse the same
instance instead of racing to build a second one or rebuilding UI on every
invocation. It deliberately does not share anything from `src/popup/`:
no pointer-relative positioning, no auto-hide on focus loss, no keyboard-grab
handling. `Behavior`/`History`/`System`/`Data` preference groups map directly
to the acceptance list in `docs/ROADMAP.md`'s Phase 6 entry; no row exists
for a setting without a concrete, already-justified use.

### Destructive operations and in-flight captures

The preferences "Clear history" action calls a new `TextHistory::clear_all`,
distinct from the popup's narrower `clear_unpinned`: it removes pinned and
unpinned items alike, including their stored images. Both bulk-clear
operations bump a `generation` counter on `TextHistory`. Image capture in
particular can span several `await` points (format negotiation, the bounded
read, blocking-pool decode/store) during which a clear could run to
completion; the clipboard service snapshots the generation before a capture
starts and compares it again immediately before the final `record`/
`record_image` call, discarding the capture (and queuing its blob for
cleanup through the existing `ImageCleanupCoordinator`) if a clear happened
in between. Without this, a capture that was already in flight when the user
cleared history could otherwise insert itself right after the clear that was
supposed to remove it.

## Positioning

This is the primary architecture risk.

### Constraints

The primary V1 target is Zorin GNOME/X11, where Phase 0 validated reliable
pointer-relative placement. GNOME Wayland does not give ordinary clients the
same global window-positioning model as X11, so native Wayland uses
compositor-managed fallback placement. Running the X11 backend through
XWayland in a Wayland session remains experimental.

### Backend boundary

Positioning logic should be isolated behind a narrow API such as:

```rust
trait PopupPositioner {
    fn place(&self, /* popup/context */) -> Result<PlacementOutcome, PositionError>;
}
```

The final API should follow what GTK/GDK actually permits; do not force this exact signature.

### Placing before the window is mapped

The popup must never be seen anywhere but at the pointer, so opening it places
the window before the compositor maps it:

1. the content is rendered first, so the placement measures the real popup;
2. the window is realized, which creates its surface without mapping it, and
   placed while it is still off screen — an already-open-once popup would
   otherwise be mapped at its previous position;
3. the placement is also written to `WM_NORMAL_HINTS` as a user-specified
   position, because a window manager places a window it has not managed yet by
   its own policy and ignores coordinates set before the first map;
4. the window is presented fully transparent and revealed on the first frame,
   after a second, authoritative placement that runs with the final mapped
   size.

The second placement reuses the pointer sample of the first one, so a pointer
that keeps moving while the popup opens cannot pull it to a second position.
Placement also runs on its own X connection, so the display is synchronised
first: a pending unmap from a previous open could otherwise be processed after
the move and leave the popup at its old position.

Steps 2 and 3 remove the visible jump; step 4 covers whatever the compositor
does in between. Placement maths, clamping, monitor selection and the fallback
path are unchanged.

### Phase 0 result

Phase 0 established these backend outcomes:

- **GNOME/X11 — working:** validated on the real target Zorin machine with
  pointer-relative placement and multi-monitor edge clamping.
- **Native GNOME Wayland — exact placement not available:** the current GTK/GDK
  path cannot choose absolute top-level coordinates, so LionClip uses a safe
  compositor-managed fallback.
- **XWayland in a Wayland session — experimental:** the isolated X11 backend is
  retained for this route, but it has not been validated in a real Wayland
  session.

The primary X11 result satisfies the V1 placement requirement. Phase 1 is not
blocked on Wayland or XWayland validation. Do not add a GNOME Shell extension
without an explicit future roadmap decision.

## Auto-paste

Phase 6 adds an opt-in (default off) setting: after restoring the selected
history item to the clipboard, also ask the application that was focused
before LionClip opened to paste it. `PasteCoordinator` (`src/paste/`) is
shaped like `Positioner` on purpose — a concrete backend picked once from
`SessionDiagnostics` and dispatched to, not a trait object — because there is
exactly one real backend (X11) and one degenerate "unavailable" case, the
same shape positioning already uses for the same reason. It extends the
existing isolated `x11rb` usage (`src/positioning/x11.rs`) rather than
building a second X11 stack: per-call connections, no long-lived state, and
key synthesis through `x11rb`'s `xtest` Cargo feature rather than shelling
out to `xdotool`/`ydotool`.

### The decision is pure; the mechanism is not

`paste::decide(auto_paste_enabled, has_target, restore_succeeded) ->
SelectionBehavior` is a free function with no GTK or X11 dependency,
covering: auto-paste off; on with a captured target and a successful
restore; on without a captured target; and a failed restore, which never
pastes regardless of the other two. Only `Enter`/click activation on a
history row calls it — navigation, pin, delete, search, the overflow menu
and opening Preferences never do, mirroring how the popup already isolates
"activation" from every other row interaction (see Popup UI above).

### Target capture: when the popup opens, not when it closes

The paste target is captured once, synchronously, at the same point the
popup's own placement pointer sample is taken — before the popup's window is
confirmed visible, so the query cannot observe LionClip's own window as
focused. It is *not* derived from whatever ends up focused after the popup
closes, which could be a different application entirely. Capture walks
`GetInputFocus` up through `QueryTree` parents to the top-level a window
manager would frame, the same tree-walk idiom `holds_keyboard_focus` in
`src/positioning/x11.rs` already uses for a related but distinct question
(whether the popup itself still owns focus).

### Fail-safe by construction

Every step either confirms real state or gives up; nothing assumes success:

1. the target's existence is re-checked at paste time, not just at capture
   time — a destroyed target fails safe;
2. activation is requested through both the standard `_NET_ACTIVE_WINDOW`
   EWMH message and a direct `SetInputFocus` reinforcement, because this is
   only ever asked to hand focus back to a window LionClip itself observed
   holding it moments earlier — the specific case focus-stealing prevention
   exists to allow, not the case it exists to block;
3. key synthesis only runs after a real `FocusIn` event confirms the target
   actually regained focus, polled non-blocking with a short pause between
   attempts and a bounded ~400 ms deadline — the event is the confirmation,
   the pause is only how often the server is asked, and a timeout fails safe
   rather than guessing;
4. Control and V key presses are synthesized through XTEST, with every
   already-pressed key unconditionally released even if a later step fails,
   so a partial failure can never leave a modifier logically stuck at the X
   server;
5. the whole validate/activate/confirm/synthesize sequence runs off the GTK
   main thread (`gio::spawn_blocking`, the same pattern image processing
   already uses), because it can take up to the confirmation deadline.

None of this ever falls back to placing the popup, hiding it, sleeping a
fixed amount and hoping a key combination landed in the right window.

### Wayland

`PasteCoordinator::is_available()` gates on the same `is_x11()` GDK-backend
check positioning already uses to decide whether to attempt its own X11
path. On Wayland the preferences switch is disabled with an explanatory
subtitle rather than hidden, so a persisted preference from an earlier X11
session stays visible; selecting a history item still restores the
clipboard exactly as it does with the setting off, and no paste is
attempted.

## Search

In-memory filtering is enough for hundreds of text entries, and that is what
Phase 3 implements (see the `HistoryQuery` note above).

FTS should only be introduced after measuring a real need.

## Concurrency

GTK UI work belongs on the GTK main context. Clipboard reads, database work, hashing large payloads, and image processing should avoid blocking rendering.

Use Rust/GLib concurrency mechanisms deliberately. Do not create a general async runtime merely because it is familiar; add one only if it materially simplifies actual requirements.

## Error handling

User-facing failures should degrade gracefully:

- unsupported clipboard content: ignore safely;
- failed history read: keep app alive and surface diagnostics without payloads;
- positioning unavailable: use fallback placement;
- corrupted/locked database: fail safely and preserve recoverability where possible.

## Logging

Logs may include:

- event type;
- content kind;
- byte size;
- timing;
- backend selected;
- error context.

Logs must not include:

- clipboard text;
- image bytes;
- secrets inferred from clipboard content.

## Desktop integration

Everything installed on a user's system lives in `packaging/` and is plain,
reviewable data or shell.

### Desktop entry

The launcher entry runs `lionclip show`, because clicking an app in the app grid
should show the app. It carries `Terminal=false`, `StartupNotify=false` and
`StartupWMClass=lionclip`.

`StartupNotify` is off on purpose: when LionClip is already running, the
launcher invocation is a remote command that exits immediately, and a startup
notification would leave the shell showing a "starting" cursor until it timed
out. `StartupWMClass` matches the `WM_CLASS` GTK sets from the program name,
verified with `xwininfo` on the target session, so the popup is attributed to
LionClip in Alt+Tab rather than to an unknown window.

### Autostart

A system-wide XDG autostart entry in `/etc/xdg/autostart` runs `lionclip`,
which starts the resident instance and its monitor and shows nothing. That is
the whole point of the bare command existing as its own case: login must start
recording without putting a window on the screen.

XDG autostart was chosen over a systemd user unit because the resident process
is a session-scoped GUI client that needs the session's display and D-Bus, which
is exactly what XDG autostart provides, and because GNOME's own tooling
(Tweaks, and a per-user copy of the file) already knows how to switch it off.

The entry is deliberately **not** a dpkg conffile, even though it lives under
`/etc`. dpkg keeps conffiles on `remove` and deletes them only on `purge`, so
declaring it would leave an autostart entry asking the session to run a
`/usr/bin/lionclip` that `remove` had just deleted. It holds no user
configuration to preserve either: the supported way to switch autostart off is a
per-user copy in `~/.config/autostart`, which is where GNOME's own tools write
it, which overrides the system entry, and which no package operation touches.

### Super+V

`lionclip-shortcut` writes a GNOME custom keybinding for `lionclip toggle`. It
is idempotent, reports what it changes, and refuses to act on two conflicts: a
custom `Super+V` shortcut belonging to another program (it stops and points at
Settings), and GNOME's own `toggle-message-tray` default (it stops unless
`--take-over` is passed). A shortcut that already runs *some* `lionclip` binary
— a development build, say — is recognized as LionClip's own and updated in
place rather than duplicated.

No GNOME Shell extension is involved.

### Packaging layout

`packaging/deb/build.sh` assembles a staging tree and calls `dpkg-deb`. There is
no packaging framework: the control file, the two maintainer scripts and the
file list are all visible in one place.

```text
/usr/bin/lionclip
/usr/bin/lionclip-shortcut
/usr/share/applications/<app-id>.desktop
/etc/xdg/autostart/<app-id>.desktop
/usr/share/icons/hicolor/scalable/apps/<app-id>.svg
/usr/share/icons/hicolor/{16,24,32,48,64,128,256}x*/apps/<app-id>.png
/usr/share/metainfo/<app-id>.metainfo.xml
/usr/share/glib-2.0/schemas/<app-id>.gschema.xml
/usr/share/doc/lionclip/{copyright,README.Debian,changelog.Debian.gz}
```

Only the schema *source* is packaged, never a compiled `gschemas.compiled`:
that file is the merge of every schema installed on the system, so shipping
one of our own would clobber every other application's compiled schemas.
`postinst`/`postrm` instead run `glib-compile-schemas
/usr/share/glib-2.0/schemas` on configure/remove/purge, the same
best-effort, tool-presence-guarded pattern already used to refresh the icon
cache and the desktop database.

Runtime dependencies are not written by hand. `dpkg-shlibdeps` reads the built
binary and produces the versioned list — GTK4, Libadwaita, GLib, GDK-Pixbuf,
Pango, libc, libgcc — so they cannot drift from what LionClip actually links
against, and no `-dev` package can leak in. SQLite is statically bundled by
`rusqlite` and correctly produces no dependency; `x11rb` speaks the X11 protocol
in Rust and links no X client library, so X11 comes in through GTK. Only
`hicolor-icon-theme` is added by hand, because the package installs into that
theme's directories.

The maintainer scripts do two things: refresh the icon cache and the desktop
database. They never start, stop or restart LionClip, and they never touch user
data. A running instance keeps the executable it started from until the user
logs out or restarts it, which `README.Debian` says plainly; killing a user's
running process from a package script would lose whatever the monitor had not
yet written.

Clipboard history is user data. `remove` and `purge` both leave
`$XDG_DATA_HOME/lionclip` alone: it can belong to several users on one machine,
and only its owner should decide to delete it.

Flatpak may be evaluated later, but sandbox/clipboard/global-shortcut constraints must be understood before making it the primary package.

### Icon

The source of truth is `packaging/icons/<app-id>.svg`, and the PNGs in the
package are rendered from it at build time by `packaging/icons/render.sh`
(`rsvg-convert`, falling back to `gdk-pixbuf-thumbnailer`). The SVG is installed
as the scalable icon; the fixed sizes exist so 16 and 24 px stay crisp in the
dock and Alt+Tab, and so the icon still appears where no SVG pixbuf loader is
installed.

![The LionClip icon at 256 px, and at 16 to 128 px on light and dark backgrounds](images/lionclip-icon-preview.png)

The drawing is LionClip's own. It follows the visual language of the sibling
Lion\* projects in the same workspace — a frontal, symmetrical lion head with a
radiating mane, a light face, and a warm gradient — and settles between the two
of them: the rounded dark tile and two-ring mane construction of LionPocket,
the orange-to-red mane of LionFlow. What makes it LionClip rather than a recolor
is the clipboard reading: the face is a squircle board instead of a circle, and
a small metal clip sits on its top edge where a lion's forehead tuft would be.
The mane is two offset rings, which is both the family's construction and the
layered-history cue.

Every element is a large flat shape, because the icon was checked at 16, 24, 32,
48, 64, 128 and 256 px; earlier drafts that put a separate clipboard object next
to the head, or a metal clip floating at the top of the tile, were dropped for
turning into noise below 32 px.
