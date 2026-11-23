# A11y MCP

A local MCP server for inspecting and editing design files with A11y's document model and code generators. By default it runs an isolated, in-memory file session over stdio. When the editor's agent bridge is on, it attaches to the running editor instead and edits the document open on screen; see [Live editor control](#live-editor-control).

## Build and connect

From the repository root:

```sh
cargo build --locked -p a11y-mcp
```

The executable is `target/debug/a11y-mcp` (`a11y-mcp.exe` on Windows). Use `--release` for a release build and change `debug` to `release` in the paths below. Rust and the repository's normal build prerequisites are required only for building.

Configure an MCP client that supports local stdio servers. These are generic `mcpServers` examples; use the configuration format and location required by your client. Replace both paths with absolute paths on your machine. The workspace directory must already exist.

Windows:

```json
{
  "mcpServers": {
    "a11y": {
      "command": "C:\\path\\to\\a11y\\target\\debug\\a11y-mcp.exe",
      "args": ["--workspace", "C:\\path\\to\\designs"]
    }
  }
}
```

macOS/Linux:

```json
{
  "mcpServers": {
    "a11y": {
      "command": "/path/to/a11y/target/debug/a11y-mcp",
      "args": ["--workspace", "/path/to/designs"]
    }
  }
}
```

The client launches the process and speaks MCP over stdin/stdout. The server does not listen on a network port. No account, API key, Figma connection, or running editor is needed; a running editor is used when available. Configuration is not installed automatically.

## File workflow

1. Call `a11y_open_file` with a path inside the configured workspace, or `a11y_new_document` with a name.
2. Use `a11y_get_document` to discover page IDs, then `a11y_list_nodes` and `a11y_get_node` to inspect layers. IDs are positive integers; obtain them from tool results rather than Figma node IDs.
3. Inspect variables or generate handoff code. Make edits with the creation, update, and deletion tools; undo and redo operate on this session's edit history.
4. Call `a11y_save_file` with an unused `.a11y` filename, or pass `"overwrite": true` to replace an existing one (for example the `path` that `a11y_get_document` reports for the file you opened). Its parent directory must already exist. Open the resulting file in the desktop editor to continue there.

Example tool arguments:

```text
a11y_open_file     {"path":"input.fig"}
a11y_get_document  {}
a11y_list_nodes    {"query":"button","limit":25}
a11y_get_node      {"node_id":42}
a11y_generate_code {"node_id":42,"language":"react"}
a11y_update_node   {"node_id":42,"name":"Primary action","width":160}
a11y_save_file     {"path":"output-v2.a11y"}
```

`42` is illustrative: substitute an ID returned by the preceding queries. Relative paths resolve beneath `--workspace`. Absolute paths are accepted only within that same directory tree. Reads canonicalize paths and reject escapes; writes resolve the existing parent. `a11y_save_file` creates a new file unless `overwrite: true` is passed, in which case it replaces an existing regular `.a11y` file atomically (temporary file + rename) and still never writes through a symlink; `a11y_export_image` overwrites an existing regular `.png` but never writes through a symlink.

Edits remain in memory until saved. Restarting the server loses unsaved work. Opening another file or starting a new document resets undo history and refuses to discard pending edits unless `discard_changes: true` is explicitly supplied. Save each revision under a new filename, or overwrite the file you opened with `overwrite: true`. Requests are handled one at a time in the order they arrive, so a batch of calls written in one go runs and answers in sequence. Tool failures return an MCP error result with an explanatory message.

Imports are limited to 64 MiB per file, 100,000 layers, and 256 levels of nesting. The server validates the imported document graph before using it, rejecting malformed parent/child relationships and cycles.

## Generating code or reading the node tree

`a11y_generate_code` answers "what should this look like"; the node tree answers "what does this project already have". Use the generator when a finished layer has to be handed off (`css` and `tailwind` for web, `compose` and `swiftui` for native, `svg` for icons), or when you want a diffable second opinion on geometry and typography. Read `a11y_get_node` and write the code yourself when the target is an existing component with its own API (a `Button / Primary` layer is a `PrimaryButton(size: md)`, not a styled container — the mapping lives in its name and its bindings, which the generator does not know), when the work spans more than one layer, or when layout semantics matter: generated code flattens auto layout, `layout_child` and constraints into absolute numbers. A practical split is to read the tree to decide what to build, generate code for layers that do not exist in the codebase yet, and write the parts that plug into existing components and tokens by hand.

[`docs/agent-workflows.md`](../../docs/agent-workflows.md) describes the loop one team runs this in — workspace conventions, save and revision habits, component and override rules — with a sanitised example document in `docs/examples/button-kit.a11y`.

## Tools

| Tool | Purpose |
| --- | --- |
| `a11y_list_files` | Design files in this user's library, plus `shared/` copies of files opened from outside it while the agent bridge is on. Each `path` is valid for `a11y_open_file`. |
| `a11y_open_file` | Import a native `.a11y`, `.fig`, `.pen` (Pencil), SVG, or supported JSON design into the session. |
| `a11y_new_document` | Start an empty document with one page. |
| `a11y_save_file` | Write the session to a native `.a11y` file: a new one, or an existing one with `overwrite: true`. |
| `a11y_get_document` | Read document name, pages, counts, undo/redo availability, and `session` (`"file"` or `"editor"`). |
| `a11y_list_nodes` | Search names case-insensitively or list direct children; paginate using `next_offset`. |
| `a11y_get_node` | Read a complete native node, including child IDs, paints, geometry, text, and bindings. |
| `a11y_get_variables` | Read collections and tokens; optionally resolve one node's direct bindings in its inherited modes. |
| `a11y_generate_code` | Return CSS, Tailwind, React, SwiftUI, Compose, or SVG source without writing a file. |
| `a11y_create_node` | Add a frame, rectangle, ellipse, or text layer to a container. |
| `a11y_update_node` | Change name, position, dimensions, visibility, opacity, or plain text. |
| `a11y_delete_node` | Remove a non-page layer and its descendants. |
| `a11y_undo` / `a11y_redo` | Traverse the edit history. In the editor it is the user's own undo stack, so `a11y_undo` may undo the user's latest edit. |
| `a11y_export_image` | Render a node to a PNG at `scale` (0.25–4, default 1) and write it to `path` inside the workspace, overwriting if present; returns `{path,width,height}`. Works in both session types. |
| `a11y_get_selection` | Read the editor's current selection. Editor only; fails in a file session. |
| `a11y_select_nodes` | Select 1–500 `node_ids` on one page in the editor (switching to that page) and, with `reveal` (default `true`), scroll/zoom to them. Editor only; fails in a file session. |

`a11y_list_nodes` defaults to 100 results and accepts 1–500 per call. `parent_id` filters direct children, not all descendants. Coordinates are relative to the parent. Width and height must be non-negative, geometry must be finite and within ±1,000,000, and opacity must be between 0 and 1. Replacing text clears rich-text runs and list ranges. Page roots cannot be edited or deleted by the layer tools.

## Live editor control

When A11y runs with its agent bridge on, `a11y-mcp` attaches to it automatically: the editor writes `bridge.json` (port and a per-launch token) into the library folder next to `settings.json` (`$A11Y_LIBRARY` honored if set), the server pings it before each call and forwards the call over loopback HTTP. `a11y_get_document` then reports `"session": "editor"` (a file session reports `"file"`) plus the open file's `path` and `has_undo`, which is true while the editor's undo stack is non-empty, including after the user saves. Edits go through the editor's undo stack, so ⌘Z in the editor and `a11y_undo` see the same history, and `a11y_undo` may undo the user's latest edit. Mutating calls are refused while a drag, pen or vector edit, dialog, or presentation is in progress ("Editor is busy"); an open text edit is committed first. A call that takes the editor longer than 5 s fails, but may still have been applied: read the document before retrying.

The first call picks the session mode and the server keeps it. In editor mode, if the bridge goes away later, calls fail with "Editor bridge is off" instead of falling back to a file session; turn the bridge on again or restart `a11y-mcp`. In file mode, an editor that appears later is used from the next call when the file session has no unsaved edits; with unsaved edits, calls fail with "An editor appeared" until you save them with `a11y_save_file` or restart `a11y-mcp`. `a11y_get_selection` and `a11y_select_nodes` fail in a file session with "No editor connected".

In this mode `a11y_open_file` opens the file in the editor (refused while the editor has undo history: ask the user to save first; `discard_changes` throws their edits away), `a11y_save_file` writes a copy to a new `.a11y` file without changing the editor's own path, and `a11y_new_document` starts a new document in the editor. Workspace rules still apply to every path. Set `A11Y_BRIDGE_FILE` to point the server at a different discovery file, or to an empty value to disable discovery entirely (used by tests).

Turn the bridge on in **Preferences → Allow MCP agents to control this editor** (remembered) or with `a11y --bridge` (this launch only). A status pill top-right shows `"Agent bridge on"` or `"Agent · <last tool>"` while it runs, with an **Off** button. The bridge never starts for `--screenshot` runs, and a second editor instance refuses to start one. It listens on 127.0.0.1 only, uses a per-launch token in the URL path and the `X-A11y-Token` header, rejects any request carrying an `Origin` header (browsers), and is removed with the pill's **Off** button or when the editor exits.

## Current limits

The server shares A11y's importers, document types, variables, undo commands, and exporters. Code generation resolves bound variable values. Import and generated-code fidelity follow the underlying A11y implementations; generated source is a handoff starting point.

In an isolated file session, layer edits do not run the desktop application's auto-layout reflow, text measurement or connector rerouting, so do not assume that resizing a container rearranges its children. Component edits do follow the editor's rules: a change to a layer inside an instance is recorded as an override on that layer, and a change to a main component re-syncs its instances, both in the same undo step. With the agent bridge on, edits run through the editor's transactions and are laid out like the editor's own edits (auto-layout reflow, group fit, connector rerouting, component-instance sync). Editing token definitions or arbitrary paint properties is not exposed in this version.

## Verification and evaluation

```sh
cargo test --locked -p a11y-mcp
```

`tests/evaluation.xml` contains ten independent, read-only retrieval questions based on the repository's fixed Figma REST fixture. For each question, start a fresh server with the repository root as `--workspace`, then preload `crates/a11y-fig/tests/fixtures/rest_file.json` using `a11y_open_file`. This preload is test setup; answering the questions requires only the read-only query/code tools. Do not create, update, delete, undo, redo, or save during evaluation. Match the response against the XML answer exactly.
