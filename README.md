# Ketchup

A small 3D edge editor written in Rust. The same editor runs as a native Windows, macOS, or Linux application and in a desktop browser through WebAssembly.

The workspace includes Select, Line, Erase, and Orbit, endpoint/midpoint/axis inference, measured lines, directional box selection, undo/redo, and local autosave. It starts with an empty model. Faces, solid modeling, file import/export, mobile, and cloud storage are outside this version.

## Prerequisites

- Rust 1.95 or newer, installed with rustup; the repository was checked with Rust 1.97.1.
- A GPU/driver supported by wgpu. The browser prefers WebGPU and falls back to WebGL2.
- On Linux, development libraries for X11/Wayland and xkbcommon. For Debian/Ubuntu: `sudo apt install build-essential pkg-config libx11-dev libxcb1-dev libxkbcommon-dev libwayland-dev libegl1-mesa-dev`.
- On Windows, the MSVC Rust toolchain and Visual Studio C++ build tools.
- On macOS, Xcode Command Line Tools.

## Run on desktop

```sh
cargo run -p ketchup-app
```

## Run in a browser

Install the browser target and Trunk once:

```sh
rustup target add wasm32-unknown-unknown
cargo install --locked trunk
```

From the repository root:

```sh
trunk serve
```

Open <http://127.0.0.1:8080>. Keep the app on the same origin/port to restore the same saved model. No Node.js, JavaScript application framework, or backend is required. Trunk generates the JavaScript needed to load the Rust WebAssembly program.

## Compile for distribution

Desktop, on the OS you want to target:

```sh
cargo build -p ketchup-app --release --locked
```

The executable is `target/release/ketchup-app` (`ketchup-app.exe` on Windows). These commands produce executables, not installers or signed application bundles. Native builds should run on the corresponding OS; this project does not configure cross-linkers or signing identities.

Browser:

```sh
trunk build --release --locked
```

Serve the resulting `dist/` directory with a static web server. Use HTTPS for WebGPU (localhost is also supported). `public_url = "./"` allows serving from a subdirectory. There is no service worker or external asset dependency.

## Controls

| Action | Control |
|---|---|
| Select | Space; click an edge, Shift-click toggles |
| Window selection | Drag left to right; selects fully enclosed edges |
| Crossing selection | Drag right to left; selects intersected edges |
| Select all edges | Ctrl/Command A, outside text fields |
| Line | L; click start and end, then continue drawing |
| Precise length | Choose a preview direction, type meters, press Enter |
| X / Y / Z axis lock | Right / Left / Up arrow; repeat to unlock |
| Clear axis lock | Down arrow |
| Cancel preview/gesture | Escape |
| Erase | E; click or drag across edges |
| Delete selection | Delete or Backspace |
| Orbit | O + left-drag, or middle-drag from any tool |
| Pan | **Hold Shift while orbiting**, including middle-drag |
| Zoom | Mouse wheel; anchors at the pointer on the camera target plane |
| Fit all geometry | F, or the Fit view button |
| Reset view | House button in the left toolbar |
| Undo | Ctrl/Command Z |
| Redo | Ctrl/Command Shift Z, or Ctrl Y |
| Shortcuts | F1 or the question-mark button |

Shift may be pressed or released during an orbit gesture. Panning changes the camera position and target together without changing camera orientation. Temporary middle-button navigation preserves the active tool and unfinished line.

Coordinates use meters and Z-up. Free drawing starts on Z=0, or at a snapped existing point. Subsequent free segments use the horizontal plane through their start; axis inference and locks allow drawing vertically. Hover markers identify endpoints, midpoints, origin, and axes. Connecting to an existing edge's midpoint splits it into connected edges. Arbitrary crossings do not split automatically; closed loops remain wireframes. Line creation uses click-to-click input, not freehand dragging.

## Saved models

The last model name, geometry, and camera are saved locally after activity settles. Geometry edits are also saved immediately in the browser. History holds the last 100 geometry operations and is not persisted. New model is undoable for geometry; naming and camera changes are outside geometry history.

- Desktop: the platform's local application-data directory, resolved by `directories::ProjectDirs`, in `model.json`.
- Browser: `localStorage["ketchup.document.v1"]`, scoped to the current origin.
- For isolated desktop sessions/tests, set `KETCHUP_DATA_DIR` to a directory of your choice.

Save failures show an Unsaved indicator with the error in its tooltip. The live model remains editable; File → Save now retries. An unreadable or unsupported document starts a clean workspace with a recovery notice. The existing saved data is not overwritten until you edit or explicitly save.

## Development and verification

```sh
cargo check --workspace --locked
cargo check --workspace --target wasm32-unknown-unknown --locked
cargo test --workspace --locked
```

An optional offscreen GPU test renders the actual UI and custom viewport to PNGs without opening a window:

```sh
cargo test -p ketchup-app gpu_renders_empty_and_modeled_workspace --locked -- --ignored --nocapture
```

Snapshots appear in `target/qa/`. This test requires a working native GPU adapter. Normal tests cover document validation, geometry, inference, camera math, selection, undo/redo, and synthetic keyboard/pointer interactions through egui.

Verified during implementation: native Linux and browser WASM Cargo checks; core and UI interaction tests; offscreen Vulkan rendering of empty, modeled, and compact workspaces. Windows/macOS execution, browser startup, and runtime WebGPU/WebGL2 fallback still require platform smoke testing. Production builds and hosted deployment were not run.

## Layout

- `crates/ketchup-core`: double-precision document geometry, inference, picking, camera, tool definitions, and bounded history. It has no GUI or GPU dependencies.
- `crates/ketchup-app`: egui workspace, gesture routing, local persistence, native/browser launchers, and the wgpu/WGSL edge renderer. Model coordinates remain independent of GPU resources.
- `index.html` and `Trunk.toml`: static browser host and WASM development configuration.

The viewport uses antialiased triangle strips for consistent edge widths, CPU projection/clipping, and the same shader and editor behavior on every target. Rendering is event-driven rather than a continuous idle loop.
