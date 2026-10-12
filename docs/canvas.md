# Canvas coordinates and viewport persistence

Drawing paths and image bounds live in document coordinates. Opening, panning,
zooming, resizing, and navigating through the minimap must never rewrite them or
request a document save. Only content edits set element `DiffState` flags.

`ViewportSettings.master_transform` maps document coordinates to egui logical
screen points. Use its conversion methods for pointer positions, vectors,
rectangles, and editing transforms. Keep selection handles, popovers, and hover
indicators in screen coordinates. Convert hit-test and eraser radii from screen
points into document units using the viewport zoom.

For a viewport matrix `V` and a screen editing operation `S`, the corresponding
document edit is `inverse(V) * S * V`. History stores this document edit, so
undo/redo does not depend on the view at the time it runs.

`Path.data` already includes element edits and imported SVG element transforms.
`Path.transform` tracks element scaling for stroke width; do not apply it again
to path positions. Serialization writes document geometry and the effective
stroke width. Image `view_box` values are also in document coordinates.

The renderer caches meshes in document coordinates and transforms copies for
painting. Main canvas, minimap, and previews can use different viewports for the
same buffer. Rebuilding a mesh, changing themes, or applying a viewport does not
produce document dirtiness. Large zoom changes can rebuild tessellation for
quality; panning reuses the existing mesh.

`WsPresistentData.canvas_files` stores device-local view state keyed by file UUID,
following Markdown's cursor/scroll persistence. It remembers zoom, the document
point at the viewport center, navigation locks, and document-space bounds.
Restore after the canvas rectangle is available. Writes are debounced, with
pending state flushed on close, replacement, or editor destruction. Read-only
drawings can still remember their view.

Older SVGs may contain `lb_master_transform` and `lb_viewport_settings`. These
are read only for compatibility and to seed local view state when none exists.
Older writers already removed the viewport from stored geometry, and baked
element edits into path data while also writing the element matrix. The parser
accounts for this legacy representation without applying the saved viewport.
Opening does not migrate document bytes. The next content save omits viewport
metadata. Reload and sync merge document-space content without changing the
local view.
