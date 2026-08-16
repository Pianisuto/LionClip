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
├── settings/
│   └── preferences.rs
└── storage/
    ├── db.rs
    └── migrations.rs
```

Do not create all of these files before their responsibilities exist. The tree is a direction, not scaffolding homework.

## Application lifecycle

Use GApplication/AdwApplication semantics for a single logical app instance.

Planned commands:

```text
lionclip
lionclip show
lionclip hide
lionclip toggle
lionclip settings
lionclip clear
```

Only add commands when the corresponding behavior exists.

A second CLI invocation should communicate with the existing application instance rather than create a second long-lived clipboard monitor.

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

## Packaging

Packaging comes after core behavior is validated.

Expected Linux integration includes:

- `.desktop` entry;
- app icon and metadata;
- XDG autostart integration where appropriate;
- documented GNOME/Zorin shortcut setup for `Super+V`;
- a reproducible `.deb` path for Ubuntu/Zorin users.

Flatpak may be evaluated later, but sandbox/clipboard/global-shortcut constraints must be understood before making it the primary package.
