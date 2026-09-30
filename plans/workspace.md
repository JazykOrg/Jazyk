# Plan: one workspace instead of three lenses

Status: proposal with an experimental build behind a switch. Written 2026-09-30. The
shipped layout stays the default until the owner picks. The docs-first rule applies:
[gui.md#workspace-experimental](../docs/frontends/gui.md#workspace-experimental) states
what the switch turns on before the code does.

## The owner's ask

- The Docs, Model, and Product split does not work. Nobody goes to Model; the model
  has to live inside the documents.
- Connections is the part that works: an entity's picture, a click on a neighbour to
  see the links and requirements between them, walking the graph one hop at a time.
  It belongs next to the text.
- Documents and Outline are in the navigator; Product belongs there too.
- The inspector column beside the chat is in the way, and on a phone it squeezes the
  center.
- An idea to try: a table top, white with light dots, where opened files are windows,
  several at once, with the connections, entities, and relationships drawn between
  them. Or tabs that split like VS Code. Or both, joined by zoom: zoom out to see all
  the windows and how they group, the graph behind them, zoom back in to one, the way
  macOS zooms out to every window.

## What is wrong with the lenses

- A lens is a place, and the model is not a place a person goes: an entity matters
  because a sentence mentions it, a requirement because a paragraph states it. The
  Model lens asks the reader to leave the sentence to see what it means.
- One center means one thing at a time: a document or an entity page, never both, so
  every look at the graph costs the document's scroll position.
- The inspector is a second center: it answers "what is this" in a column that takes
  width from the text and folds the center on a phone.
- The Product lens repeats the navigator's shape for a third tree.

## The model: items, a workspace, two arrangements

Everything the app shows is an item: a document, a product file, a generated page, an
entity, a requirement, a diagram, the product overview, the trace matrix, a history
entry, the Inbox. An item is a path in the one page table (`src/routes/AppRoutes.tsx`)
with a title and a kind. Every page already renders against a path, so an item renders
anywhere: in a tab, in a board window, in a peek.

The workspace is the set of open items and where each sits. It has two arrangements
over the same set:

- Tabs, zoomed in: editor groups with tabs, split right or down, like VS Code. The
  default candidate.
- Board, zoomed out: the same items as windows on a dotted table top, with the graph
  drawn between them. The experiment.

Moving between them is zoom, not navigation: zooming out of the tabs lays their items
out on the board; zooming into a window returns to the tabs with it in front.

## The navigator

One column, the same in both arrangements:

- Documents: the tree (folders, the row marks, the actions on the rows).
- Outline: the open document's sections, following the cursor.
- Product: `Overview` first, then the product's file tree, `Trace`, `Tests`.
- Diagrams: the views, collapsed by default.
- Generated: the generated pages, collapsed by default.

There is no Model head. Entities and requirements are reached from where they matter:
a mention in the text, a marker in the margin, the Connections panel, a diagram box,
search. The pages they open are the same entity and requirement pages as today.

## Connections replaces the inspector

The inspector column goes. What it showed is the Connections panel, docked at the
bottom of the center like VS Code's panel, following the selection:

- The cursor on a mention, or a click on a mention, a margin marker, a diagram box,
  a search result: the panel shows that node's page compactly. For an entity it is the
  connections picture; clicking a neighbour shows the links and requirements between
  the two, as today; `Open` opens the node as a tab (or a window on the board).
- The panel keeps its height and its open state per browser; it collapses to a
  strip. It never takes width from the text.
- On a phone it is a bottom sheet over the editor, dismissed by a swipe.

## Tabs

- Editor groups with tabs; split right or down from a tab's menu or by dragging a tab
  to an edge; a group closes with its last tab.
- A single click in the navigator opens a preview tab (its title in italics) that the
  next single click replaces; a double click, an edit, or a pin keeps it.
- The address bar is the focused tab's path, so back, forward, and a shared link work.
- The open tabs and the splits are remembered per project and come back on a reload.
- On a phone: one tab in view and a tab switcher, which is the board's overview laid
  out as a grid.

## Board

A table top, white with light dots, that pans and zooms without end.

- Windows: every open item is a window with a title bar to drag, a resize corner, and
  a close. A window renders its page at full size and is live (an editor edits) when
  the zoom is near 100 percent.
- The graph layer (a toggle, on by default): the entities the open windows mention or
  define appear as chips beside them; relationships between those entities are drawn
  as edges with their verb; a document window links to the chips of the entities it
  defines; a requirement links to the product file that implements it when both are
  on the board.
- Opening from inside a window (a mention, a link, a neighbour in Connections) places
  the new window beside the one it came from, with an edge between them, and pans to
  it. Nothing opens on top of the text a person was reading.
- The layout is the person's: windows stay where they are dragged, remembered per
  project; `Arrange` lays the windows and chips out by the graph (layered, relations
  flowing one way).
- A minimap in a corner.

### Zoom levels

The board changes what it draws as it zooms, so every level stays readable:

| Zoom | Windows | Graph |
|---|---|---|
| near 100% | live pages, editable | chips and edges beside them |
| 40 to 80% | still renderings of the pages; a click zooms to the window | same |
| 15 to 40% | cards: title, kind, a few lines, status | chips named, edges with verbs |
| under 15% | groups: documents by folder, entities by the component that holds them, each a labeled region with counts | edges between groups, lifted like the diagrams lift them |

- Double click a window or a group: zoom to it. `Escape`: zoom out one level. `Z`:
  fit everything. Pinch or Ctrl and the wheel zoom; Space and a drag pan.
- The overview is the macOS gesture: every window and every group at once, and a
  click on one zooms into it.

### Cost

A live editor per window is heavy. At most six windows are live at a time, the ones
nearest the center of the view; the rest render still until they are approached.

## The zoom bridge

- From the tabs, zooming out (the Overview button, pinch out, or `Ctrl -` past the
  smallest step) lays the open tabs out on the board as windows, keeping their groups
  as neighbourhoods, and animates to them.
- From the board, a double click on a window zooms into the tabs with that item's tab
  in front.
- The two share one set of open items: closing a window closes its tab.

## The top bar

The project menu, search, Inbox, History, the Build button, settings, and a
Tabs or Board switch. No lenses. The Developer pages open as tabs from a Developer
menu while the toggle is on.

## Phases

1. Done: every page renders against its own path (`AppRoutes` with `location`), so
   a tab or a window can show any page.
2. Tabs behind the switch: the unified navigator, editor groups with tabs and splits
   (the `dockview` library), preview tabs, the address bar synced, tabs restored,
   the Connections panel in the inspector's place, the phone's bottom sheet and tab
   switcher.
3. Board behind the switch: the table top (the `@xyflow/react` library: pan, zoom,
   dotted background, minimap, edges), windows over `AppRoutes`, the graph layer, the
   zoom levels, groups from the component tree, the remembered layout, `Arrange`.
4. The zoom bridge between the two.
5. The owner tries both, picks the default, and the lenses and the Model pages as a
   destination go.

## Open questions for the owner

- Does the Connections panel follow the cursor (it changes as the caret moves over
  mentions) or only a click? Recommended: a click, with the cursor's entity shown as
  a hint in the panel's title, so reading is never interrupted.
- Is the board a workspace of its own, remembered apart from the tabs, or always the
  tabs zoomed out? Recommended: one set of items, two arrangements, so nothing is
  open in one and missing in the other.
- Which is the default once both exist? Recommended: tabs, with the board one zoom
  away.
