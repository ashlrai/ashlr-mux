# Custom sidebars: vibe-code your own cmux sidebar

cmux lets you build your own sidebar UI by writing a small SwiftUI-style file.
It is interpreted at runtime (no Xcode, no build step, no signing), renders as
native SwiftUI in the real sidebar, hot-reloads on save, binds to live cmux
state, and can run cmux commands on tap. This guide is the authoring contract
for you or a coding agent.

It is a beta, on by default. Turn it off in **Settings → Custom Sidebars**
(`customSidebars.beta.enabled`). While off, custom sidebars do not appear.

## If you are an agent building this for someone

Assume the person asking is not technical. They are describing a result ("a
sidebar that shows my workspaces and lets me jump between them"), not an
implementation. Your job is to turn that into a clean, native-looking, working
sidebar and make the engineering decisions for them. Do not ask them about
SwiftUI, files, or syntax. Concretely:

- Default to real, live data. If they mention workspaces/tabs, bind to the
  `workspaces` context (not hard-coded text) so it stays correct on its own.
- Make it interactive by default. Rows that represent something you can open
  should be tappable and run the matching `cmux(...)` action (e.g. selecting a
  workspace, focusing a tab). A list that just displays text is rarely what
  they wanted.
- If the list is something a person would naturally reorder (workspaces, tasks,
  a queue), make it drag-and-drop reorderable with `Reorderable` (see below).
  When in doubt for a workspace list, prefer `Reorderable`.
- Keep it native and uncluttered: a title, a divider, then the content. Use the
  status dot / pill / highlight patterns below so it is scannable at a glance.
- Lazy-load / cap large lists (see Performance). Do not render hundreds of rows.
- Iterate by saving the file and opening it as a pane with
  `cmux sidebar open <name>`; it hot-reloads there while you edit. Verify it
  shows real data and that taps do the right thing before declaring it done.
- Stay inside the supported subset below. If something is not supported, choose
  the closest supported approach rather than failing.

## Where to put a sidebar

Write a named file (the name becomes the menu label; use short kebab-case):

    ~/.config/cmux/sidebars/<name>.swift     # interpreted Swift (preferred)
    ~/.config/cmux/sidebars/<name>.json      # declarative JSON (simpler, static)

Each file shows up as an option in the **sidebar toggle button's right-click
menu** and can also open as a normal Bonsplit pane tab. Pick it from the menu
for the left sidebar, or run `cmux sidebar open <name>` to show it in a pane;
edit the file and save and it hot-reloads. If both `<name>.swift` and
`<name>.json` exist, `.swift` wins.

Optional capability manifests live next to the sidebar as
`<name>.manifest.json`. Windows/Tauri validation reports these manifests and
splits requested methods into safe-default and denied sets. Manifests are
discoverability/trust metadata today; they do **not** grant dangerous methods by
themselves.

```json
{
  "trusted": false,
  "capabilities": ["workspace.select", "browser.eval"]
}
```

If an authored action is denied, the inline error includes manifest-requested
methods that are still blocked by the safe default policy.
When a custom sidebar is selected in the left-sidebar host, Windows/Tauri also
shows a compact manifest strip: no-manifest sidebars display the safe default
policy, valid manifests show requested/allowed method counts, and denied
requests are called out before an author triggers the blocked action.

A sidebar file is a single SwiftUI-style view expression (no `struct`, no
`var body` wrapper, just the view).

## Choosing the renderer (in-process vs remote)

By default a custom sidebar renders in-process: the interpreted view mounts
as real SwiftUI inside the cmux window, so hover styling, focus, keyboard,
and same-frame resize all work natively. The tradeoff is that the
interpreter shares the host process.

For sidebars from sources you do not fully trust you can switch to the
remote renderer, an out-of-process worker. That is the containment lane: a
crash or hang caused by the interpreted file cannot take down cmux, but
input is limited to forwarded clicks (no hover, focus, or keyboard).

Set it in **Settings → Custom Sidebars**, or in `~/.config/cmux/cmux.json`:

    { "customSidebars": { "renderer": "remote" } }

Valid values are `"inProcess"` (default) and `"remote"`. The setting is read
live; flipping it re-renders the selected sidebar without a restart. Both
renderers protect the host against pathological sources with an evaluation
budget (nesting depth and total produced nodes): a render that exceeds the
budget is discarded and the last good render stays up.

## Downloadable examples

The repo includes ready-to-copy sidebars in `Examples/CustomSidebars/`:

- `status-board.swift` groups workspaces by live signals like urgent bugs,
  review, progress, research, and done.
- `finder.swift` shows a macOS Finder-style workspace browser with a source
  list, selected workspace details, and tabs.

Install one from a cmux checkout:

    mkdir -p ~/.config/cmux/sidebars
    cp Examples/CustomSidebars/status-board.swift ~/.config/cmux/sidebars/status-board.swift
    cp Examples/CustomSidebars/finder.swift ~/.config/cmux/sidebars/finder.swift

Then validate and open it as a Bonsplit pane:

    cmux sidebar validate status-board
    cmux sidebar open status-board

Windows/Tauri status: `cmux sidebar list` and `cmux sidebar validate [name]`
now inspect `~/.config/cmux/sidebars` and return structured validation results.
`cmux sidebar open <name>` validates the sidebar and binds the focused pane to a
`custom-sidebar` surface backed by live session data. `.json` sidebars render in
that pane with the block schema below, including interactive workspace and tab
rows plus authored action objects that run through the same dispatcher as the
control socket. `.swift` sidebars render through a constrained Windows/Tauri
Swift subset renderer that supports common `VStack`/`HStack`/`Text`/`Divider`/
`Spacer`/`Button`/`Label`/`Image`/`AsyncImage`/`ProgressView`/`Gauge`/`ScrollView`/`List`/
`Section`/`ZStack`/`Grid`/`GridRow`/`Menu`/`ViewThatFits`/`AnyView`/
`TextField`/`SecureField`/`TextEditor`/`Toggle`/`Slider`/`Stepper`/`DatePicker`/`ColorPicker`/`Picker`
authoring, sidebar-local `Form { ... }` containers, basic shapes (`Circle`, `Ellipse`, `Rectangle`, `RoundedRectangle`,
`UnevenRoundedRectangle`, `Capsule`, `ContainerRelativeShape`, and constrained
`Path(roundedRect:)` / `Path(ellipseIn:)`), stack `spacing:`, `Button(role:)`,
CSS-backed SwiftUI gradient styles (`LinearGradient`, `RadialGradient`,
`AngularGradient`, and `Color.<token>.gradient`),
CSS-backed Material background tokens (`.ultraThinMaterial`, `.thinMaterial`,
`.regularMaterial`, `.thickMaterial`, `.ultraThickMaterial`, and `.bar`),
`.onTapGesture { cmux(...) }`, `.onTapGesture(count:) { cmux(...) }`,
`.onLongPressGesture { cmux(...) }`,
`.onSubmit { ... }`, `.onChange(of: value) { ... }`,
`.onAppear { ... }`, `.onDisappear { ... }`, `.task { ... }`, `.task(id:) { ... }`,
`.onHover { hovering in ... }`, `.onGeometryChange(for:) { ... }`,
`.focusable()`, `.focused($localBool)`,
`ForEach(workspaces)`,
`ForEach(workspaces.indices)`, constrained `ForEach($localCollection)` element
bindings for local `@State` arrays, simple `for` ranges, `if let` optional binding,
local `let` bindings, simple user `func` value helpers and `some View` row
helpers, `workspaces[i]` subscript reads, array helpers (`.first`, `.last`, `.contains`,
`.reversed()`, `.prefix(n)`, `.suffix(n)`, `.dropFirst(n)`, `.dropLast(n)`,
`.enumerated()`, `.filter { ... }`, `.map { ... }`, `.flatMap { ... }`,
`.compactMap { ... }`, `.reduce(initial) { ... }`, `.sorted { ... }`,
`.min(by:)`, `.max(by:)`, `.allSatisfy(...)`,
constrained key-path transforms such as `.map(\.title)`,
`.compactMap(\.progress)`, `.flatMap(\.ports)`, and `.sorted(by: \.title)`),
constrained `Dictionary(grouping: workspaces, by: \.selected)` records,
and `ForEach(Array(workspaces.enumerated()), id: \.offset)`
tuple-style closure params plus `$0`/`$1` shorthand closures, string helpers (`.count`, `.hasPrefix`,
`.hasSuffix`, `.contains`, `.uppercased()`, `.lowercased()`,
`.split(separator:)`), common numeric/string builtins (`min`, `max`, `abs`,
`Int`, `Double`, `String`), numeric formatting (`.formatted(.currency(code:))`,
`.formatted(.percent)`, `.formatted(.notation(.compactName))`, and
`Text(value, format: ...)`), arithmetic/comparison/logical expressions
(`+ - * / %`, `== != > >= < <=`, `&& || !`), and nested Swift string
interpolation, `Text(verbatim:)`, static inline `Text` markdown for
`**bold**`, `*italic*`, `` `code` ``, and `[label](https://...)` links,
constrained `Text("A") + Text(value)` concatenation,
dictionary literals (`["key": value]`), dynamic keyed
subscripts (`lookup[workspace.id]`), and optional-style missing-value checks
against `nil`. It also applies a constrained visual modifier
subset: `.font`, `.fontWeight`, `.fontDesign`, `.fontWidth`, `.dynamicTypeSize`, `.bold`,
`.foregroundColor`/`.foregroundStyle`, `.italic`, `.monospaced`, `.lineLimit`, `.truncationMode`,
`.multilineTextAlignment`, `.textCase`, `.underline`, `.strikethrough`,
`.opacity`, `.hidden`, `.fixedSize`, `.badge`, `.allowsHitTesting`, `.hoverEffect`,
`.defaultHoverEffect`, `.disabled`, `.help`, `.accessibilityLabel`,
`.accessibilityHidden`, `.accessibilityValue`, `.accessibilityHint`,
`.accessibilityAddTraits`, `.accessibilityElement(children:)`,
`.accessibilityAction`, `.accessibilityActivationPoint`, `.accessibilityRepresentation`,
`.accessibilitySortPriority`, `.redacted`, `.privacySensitive`, `.padding`, `.background`,
`.safeAreaPadding(...)`, `.contentMargins(...)`, `.cornerRadius`, `.layoutPriority`, `.containerRelativeFrame(...)`,
`.coordinateSpace(name:)`, `.alignmentGuide(...)`,
`.offset`, `.position`, `.zIndex`, `.aspectRatio`,
`.scaledToFit`, `.scaledToFill`, `.clipShape`, `.clipped`, `.shadow`,
`.border`, `.stroke`, `.blur`, `.brightness`, `.contrast`, `.saturation`,
`.grayscale`, `.hueRotation`, `.blendMode`, `.rotationEffect`, `.scaleEffect`,
`.rotation3DEffect`, `.visualEffect`, `.listStyle`,
`.labelStyle`, `.labelsHidden`, `.menuStyle`, `.controlGroupStyle`, `.controlSize`, `.buttonStyle`,
`.buttonBorderShape`, `.pickerStyle`, `.tabViewStyle`, `.toggleStyle`, `.textFieldStyle`,
`.scrollContentBackground`, `.scrollIndicators`, `.scrollClipDisabled`,
`.scrollTargetBehavior`, `.scrollTargetLayout`, `.scrollBounceBehavior`,
`.scrollDisabled`, `.scrollPosition`, `.defaultScrollAnchor`,
`.resizable(capInsets:resizingMode:)`, `.renderingMode`, `.interpolation`,
`.antialiased`, `.flipsForRightToLeftLayoutDirection`, `.imageScale`, `.symbolRenderingMode`, `.symbolVariant`,
`.listRowBackground`, `.listRowSeparator`, `.fill`, `.tint`,
`.animation(...)`, `.transition(...)`, `.contentTransition(...)`,
`.symbolEffect(...)`, `.symbolEffectsRemoved()`, `.preferredColorScheme(...)`,
`.environment(\.colorScheme, ...)`, `.environment(\.layoutDirection, ...)`,
`.frame(maxWidth: .infinity)`, and child-bearing `.background { ... }`,
`.overlay { ... }`, `.mask { ... }`, `.safeAreaInset { ... }`,
`.contextMenu { ... }`, `.refreshable { ... }`, `.swipeActions { ... }`, and
`.accessibilityRepresentation { ... }` wrappers.
Static chrome/semantics modifiers `.navigationTitle(...)`,
`.navigationSubtitle(...)`, `.navigationBarTitleDisplayMode(...)`,
`.toolbar { ToolbarItem { ... } }`, `.toolbarBackground(...)`,
`.toolbarColorScheme(...)`, `.keyboardShortcut(...)`,
`.keyboardShortcut(..., modifiers: [.command, .shift, .option, .control])`,
`.contentShape(...)`, `.allowsHitTesting(...)`, `.id(...)`, static string `.draggable(...)`, and
`.dropDestination(for:) { cmux(...) }` are rendered as sidebar-local
metadata/chrome. Static toolbar content preserves constrained
`ToolbarItem(placement:)` tokens as stable placement classes/data, and
constrained toolbar background/color-scheme tokens as sidebar-local nav/toolbar
chrome hints plus `data-swift-toolbar-*` breadcrumbs; it does not claim native
platform toolbar slotting, collapsing, color propagation, visibility resolution,
or customization semantics.
Constrained `.labelsHidden()` preserves authored label-hiding intent as stable
`data-swift-labels-hidden` metadata and applies sidebar-local visually-hidden
label chrome for common controls; full inherited SwiftUI label-style propagation
and exact platform label layout remain follow-up.
Constrained `.controlGroupStyle(...)` preserves built-in style tokens as stable
`data-swift-control-group-style` metadata and sidebar-local grouped-control
chrome hints; custom `ControlGroupStyle` structs and native platform menu/palette
behavior remain follow-up.
Constrained `.contentShape(...)`, counted tap gestures, and long-press gestures
emit stable Swift-style shape/gesture/count/duration breadcrumbs; they do not
claim native SwiftUI hit-test geometry, gesture priority, composition, or
gesture-value semantics.
Constrained `.allowsHitTesting(...)` preserves authored hit-testing intent as
stable `data-swift-allows-hit-testing` metadata and maps `false` to a
sidebar-local `pointer-events:none` hint; native SwiftUI hit-test tree behavior,
gesture priority, and keyboard-vs-pointer focus nuance remain follow-up.
Constrained `.hidden()` preserves authored hidden-view intent as stable
`data-swift-hidden` metadata and maps to sidebar-local `visibility:hidden`, so
layout space is retained while visible chrome is suppressed; SwiftUI identity,
transition, lifecycle, and native layout negotiation semantics remain follow-up.
Constrained `.badge(...)` preserves scalar/text badge values as stable
`data-swift-badge` metadata and renders a sidebar-local pill on the authored
node; native list-row, tab, toolbar, menu, and accessibility badge placement
semantics remain follow-up.
Constrained `.hoverEffect(...)` and `.defaultHoverEffect(...)` preserve authored
pointer-hover style tokens as stable `data-swift-hover-effect` /
`data-swift-default-hover-effect` metadata and sidebar-local hover chrome hints;
native pointer regions, inherited default hover environments, and exact platform
hover-effect animations remain follow-up.
Static `.draggable(...)` resolves string/scalar payloads into
browser draggable affordances plus `data-swift-draggable` breadcrumbs; it does
not claim native `Transferable`, item-provider, preview, or drag-session
semantics. `.id(...)` is exposed as `data-swift-id` for
author/debug identity breadcrumbs, but it does not yet force SwiftUI-style view
replacement semantics. `.preferredColorScheme(.dark|.light)` plus constrained
`.environment(\.colorScheme, ...)` and `.environment(\.layoutDirection, ...)`
emit safe CSS/data hints on the authored node; full inherited SwiftUI
environment propagation and arbitrary environment keys remain follow-up.
Constrained read-only `@Environment(\.colorScheme)`,
`@Environment(\.layoutDirection)`, and `@Environment(\.locale)` declarations are
available to sidebar expressions, branches, and action params using stable
sidebar defaults; host/user preference propagation and scene/dismiss environment
values remain follow-up. `.flipsForRightToLeftLayoutDirection(...)` preserves
authored image-mirroring intent as stable classes/data plus a bounded CSS mirror
hint; inherited native RTL image/layout behavior remains follow-up.
Keyboard shortcuts render as safe `aria-keyshortcuts` metadata plus stable
modifier classes; they do not register native/global accelerators.
`ControlGroup { ... }` renders as a compact sidebar-local grouped-control
container with stable classes/data breadcrumbs; it does not claim native
segmented controls, toolbar grouping, or platform control-group styling.
`GroupBox("Title") { ... }`, `GroupBox { ... }`, and constrained
`GroupBox(label: { ... }) { ... }` forms render as sidebar-local labeled card
containers with stable classes/data breadcrumbs. Constrained
`.groupBoxStyle(...)` tokens are preserved as sidebar-local style hints; custom
`GroupBoxStyle` structs and native platform group-box chrome remain follow-up.
`DisclosureGroup("Title") { ... }`, `DisclosureGroup { ... }`, and constrained
`DisclosureGroup(isExpanded: $localBool) { ... } label: { ... }` forms render as
sidebar-local collapsible sections with stable classes/data breadcrumbs. Direct
local `@State Bool` expansion bindings sync through the existing sidebar state
engine; native `DisclosureGroupStyle`, inherited style environments, and exact
SwiftUI outline/list integration remain follow-up.
`Form { ... }` renders as a sidebar-local form/list container with stable
classes/data breadcrumbs and existing interpreted controls inside; native
platform form row chrome, grouped styling, edit mode, and broader form-specific
behaviors remain follow-up.
`NavigationStack`/`NavigationLink` support a
sidebar-local push stack for static destination closures and a constrained
`NavigationLink(value:)` plus `.navigationDestination(for:) { value in ... }`
route-matching subset for string-like values. `path:` bindings,
`navigationDestination` overloads beyond this value subset, `dismiss`, and
`NavigationSplitView` remain follow-up work.
`Link("Title", destination: URL(string: "https://...")!)` and
`Link(destination: URL(string: "https://...")!) { <label> }` render as
sidebar-local external links for safe `http`/`https` destinations with stable
classes/data breadcrumbs. Unsafe or unsupported schemes render as inert disabled
link rows; native `OpenURLAction`, custom scheme dispatch, and platform URL
handling policy remain follow-up.
`ContentUnavailableView("Title", systemImage: ..., description: Text(...))` and
constrained label/description/actions builder forms render as sidebar-local
empty-state panels with stable classes/data breadcrumbs. Native platform
`ContentUnavailableView` styling, search-specific initializers, localization,
and broader symbol/style semantics remain follow-up.
Constrained `.searchable(text: $localQuery, placement: .sidebar, prompt: "...")`
renders sidebar-local search chrome with stable classes/data breadcrumbs and
direct local `@State String` write-back through the existing sidebar state
engine. Native search suggestions, tokens, scopes, platform search-field
placement, and broad `SearchFieldPlacement` semantics remain follow-up.
Presentation modifiers `.sheet(isPresented:)`, `.popover(isPresented:)`,
`.fullScreenCover(isPresented:)`, `.alert("Title", isPresented:)`, and
`.confirmationDialog("Title", isPresented:)` support a sidebar-local panel for
simple local `@State Bool` bindings. `.sheet(item:)`, `.popover(item:)`,
`.fullScreenCover(item:)`, `.alert("Title", item:)`, and
`.confirmationDialog("Title", item:)` also support a constrained
optional/string-like item subset where the item value is bound into the content
closure. Closing a Bool presentation flips the local state binding to `false`;
closing a local item presentation clears the item binding to `null`.
`Button("Close") { dismiss() }` and `Button(action: { dismiss() }) { ... }`
also work inside sidebar-local presentation content as a constrained close
command. Alert and confirmation dialog bodies render direct child
`Button(role: .destructive/.cancel)` views as dialog action rows with role
styling. `.presentationDetents([.medium, .large])`,
`.presentationDragIndicator(.visible/.hidden)`,
`.presentationBackground(...)`, and `.presentationCornerRadius(...)` on
presented content are hoisted to sidebar-local panel chrome classes, safe CSS
background/radius hints, and `data-swift-presentation-*` breadcrumbs. Native
window/modal semantics, full `@Environment(\.dismiss)` propagation, full
Identifiable item semantics, native material sampling, adaptive presentation
behavior, and native detent height negotiation remain follow-up work.
`@State var name = ...` declarations support direct local `$name` bindings for
editable `TextField`, `SecureField`, `TextEditor`, `Toggle`, `Slider`,
`Stepper`, `DatePicker`, `ColorPicker`, and simple text-option `Picker`
controls inside the sidebar pane. Constrained `ForEach($items) { $item in ... }`
loops over local JSON-like `@State` arrays also support editable element-field
bindings such as `$item.title`, `$item.done`, and `$item.count` for those direct
controls by writing back through the local array index. `Picker` options may use
constrained `.tag(value)` metadata on text/label-like option rows so the visible
label can differ from the written selection value. Bindings to live cmux/session data and
`.constant(...)` still render accessible read-only controls; attach
`.onTapGesture { cmux(...) }` or `.onLongPressGesture { cmux(...) }` when a
control-shaped row should trigger a backend action. Direct local controls also
support `.onSubmit { ... }` and `.onChange(of:) { ... }` for simple local
`@State` assignments plus safe-scoped `cmux(...)` calls. Rendered nodes also
support constrained `.onAppear { ... }` and `.onDisappear { ... }` lifecycle
hooks plus `.task { ... }` / `.task(id:) { ... }` mount/id-change hooks for the
same safe local handler subset. Single-line text fields submit on Enter;
multiline text editors submit on Ctrl/Cmd+Enter. This is not a full Swift
closure executor, async task runtime, value-diff engine, or exact SwiftUI
lifecycle model. `.onHover { hovering in ... }` supports constrained mouse
enter/leave local state updates and safe actions, but not full gesture
composition, drag/drop values, or native pointer-region semantics.
`.focusable()` makes a rendered node keyboard-focusable, and
`.focused($localBool)` mirrors browser focus/blur into a direct local `@State`
Bool binding; full `@FocusState`, programmatic focus ownership, focus scopes,
and native focus propagation remain follow-up. Use
`.onEvent("workspace.selected") { localState = events.latest.name }` or
`.onEvent(category: "workspace") { ... }` for event-driven local `@State`
assignments; handler bodies may also call `cmux(...)` through the same
safe-scoped action dispatcher.
It captures authored `cmux(...)` actions; unsupported SwiftUI is skipped with
inline warnings.
Authored JSON and Swift actions run through a safe default capability scope:
sidebar navigation/validation, workspace selection, surface focus/navigation,
sidebar read methods, and sidebar presentation metadata updates are allowed;
browser automation, debug methods, remote/SSH configuration, close/delete
commands, and broad file/system mutations are denied with
`custom_sidebar_capability_denied`. Safe-default authored actions also pass a
method-specific schema gate before dispatch; missing/wrong required fields fail
with `custom_sidebar_action_schema_invalid`, including the accepted parameter
keys for the method. Adjacent `<name>.manifest.json` files are included in
validation output and denied-action data so authors can see which requested
methods are safe by default and which remain blocked.
`cmux sidebar reload [name]` now validates sidebars and emits a
`cmux://custom-sidebar-reload` event; open authored sidebar panes poll their
source file so saved edits hot-reload. `cmux sidebar select <name>` validates
the sidebar, emits `cmux://custom-sidebar-select`, and activates the
left-sidebar custom host.

`cmux sidebar select <name>` previews a custom sidebar in the left sidebar.
Use `cmux sidebar open <name>` when you want the sidebar as a normal pane tab
that can live in a right-side split.

## Quick start

    cat > ~/.config/cmux/sidebars/mine.swift <<'SWIFT'
    VStack(alignment: .leading, spacing: 8) {
        Text("My sidebar").font(.title3).bold()
        Text(clock.time).font(.caption).foregroundColor(.secondary)
        Divider()
        ForEach(workspaces) { w in
            Button(action: { cmux("workspace.select", workspace_id: w.id) }) {
                HStack {
                    Text(w.selected ? "●" : "○").foregroundColor(w.selected ? "#FF8800" : .secondary)
                    Text(w.title)
                    Spacer()
                }
            }
        }
    }
    SWIFT

Then right-click the sidebar button and choose **mine**, or open it as a pane
with:

    cmux sidebar open mine

## JSON sidebars

The JSON form is a small declarative renderer for simple, static layouts that
still bind to live cmux session data. On Windows/Tauri, `cmux sidebar open
<name>` renders `.json` files directly in the custom-sidebar pane.

Example:

```json
{
  "title": "Board for {selectedTitle}",
  "subtitle": "{workspaceCount} workspaces, {unreadTotal} unread",
  "blocks": [
    { "type": "heading", "text": "Live board" },
    { "type": "stat", "label": "Open ports", "value": "{portTotal}" },
    {
      "type": "button",
      "label": "Mark {selectedTitle}",
      "detail": "Runs workspace.set_status",
      "action": {
        "method": "workspace.set_status",
        "params": { "key": "json", "value": "from {sourceName}" }
      }
    },
    { "type": "workspaceList", "title": "Unread", "filter": "unread" },
    { "type": "selectedTabs", "title": "Tabs" }
  ],
  "footer": "Rendered from {sourceName}"
}
```

Supported placeholders in `title`, `subtitle`, `footer`, `heading`, `text`,
`stat`, and action parameter strings: `{sourceName}`, `{workspaceCount}`,
`{selectedTitle}`, `{selectedId}`, `{unreadTotal}`, and `{portTotal}`. Row
actions can also use `{workspace.id}`, `{workspace.title}`, `{workspace.index}`,
`{tab.id}`, and `{tab.title}`.

Supported block types:

- `{ "type": "heading", "text": "..." }` renders a section heading.
- `{ "type": "text", "text": "..." }` renders body copy.
- `{ "type": "divider" }` renders a separator.
- `{ "type": "stat", "label": "...", "value": "..." }` renders a key/value stat row.
- `{ "type": "button", "label": "...", "detail": "...", "action": { "method": "workspace.set_status", "params": { ... } } }` renders a button that invokes any supported cmux socket method through the shared dispatcher.
- `{ "type": "workspaceList", "title": "...", "filter": "all|selected|unread|ports|dirty|remote", "limit": 20 }` renders workspace rows. Clicking a row runs `workspace.select` unless `"action": "none"` is set. `"action"` can also be an action object with a `method` and `params`.
- `{ "type": "selectedTabs", "title": "...", "limit": 20 }` renders tabs for the selected workspace. Clicking a row runs `surface.focus` unless `"action": "none"` is set. `"action"` can also be an action object with a `method` and `params`.

## Live data you can bind to (read-only, refreshes on state/events)

- `workspaces` — array, one per workspace. Always present: `id`, `title`,
  `selected` (Bool), `pinned` (Bool), `index` (Int), `directory`, `ports`
  (array of Int) + `portCount`, `unread` (Int notifications), `tabs` + `tabCount`.
  Present when the workspace has them (use `if let` / ternary): `description`,
  `color` (hex), `branch` + `dirty` (Bool) from git, `pr`
  (`{ number, label, url, status: open|merged|closed, stale, branch }`, the
  workspace's first pull request in sidebar display order) + `prs` (array of
  the same shape with every pull request cmux knows for the workspace),
  `progress` (`{ value: 0..1, label }`), `latestMessage` (last agent message),
  `latestPrompt` (last submitted prompt), `latestAt` (epoch), `remote`
  (`{ target, state, connected }`).
- `tabs` (per workspace) — array of surfaces. Always: `id`, `title`,
  `focused` (Bool), `pinned` (Bool). When available: `directory`, `branch` +
  `dirty`, `ports` (array of Int).
- `workspaceCount` — Int. `selectedTitle` — active workspace's title.
  `selectedId` — its id. `unreadTotal` — total unread notifications.
- `events` — recent event-stream context for reactive sidebars and external
  tools. Includes `latest`, `recent` (bounded retained tail), `category_counts`,
  `name_counts`, `latest_seq`, `oldest_seq`, `next_seq`, and `boot_id`. Use
  `events.latest` for the most recent trigger. On Windows/Tauri, open authored
  sidebars seed this from `extension.sidebar.snapshot` and update it from the
  in-process event bridge whenever cmux records a new event; external tools can
  still subscribe with `cmux events --cursor-file ... --reconnect`.
  Convenience aliases `latestEventName`, `latestEventCategory`, and
  `latestEventSeq` are available for JSON templates.
  Swift sidebars can also attach `.onEvent("event.name") { ... }` or
  `.onEvent(category: "workspace") { ... }` to any rendered view. Windows/Tauri
  skips the retained bootstrap event and runs matching handlers for subsequent
  live events; supported handler statements are local `@State` assignments and
  safe-scoped `cmux(...)` calls.
- `clock` — `{ time ("HH:mm:ss"), hour, minute, second, weekday, epoch }`. The
  sidebar re-renders about once a second, so clocks/countdowns and workspace
  changes are live.

Optional fields are omitted when the workspace doesn't have them, so guard with
`if let b = w.branch { ... }` or `w.pr != nil ? ... : ...` rather than assuming
they exist.

To inspect exactly what the current desktop session exposes to custom sidebars,
run `cmux sidebar-snapshot --json`. This prints the same
`extension.sidebar.snapshot` payload used for event-stream catch-up and includes
an interpreter-ready `data` object containing the friendly aliases shown above,
plus raw snake_case runtime fields for tooling that needs backend-level detail.
On Windows/Tauri, open authored sidebar panes receive the same event records
in-process; `cmux events --category session --category workspace --category pane
--category surface --category sidebar --no-heartbeat --reconnect` provides
retained and live frames for external tools that want to refresh or reduce that
snapshot.

## Views

Containers: `VStack(alignment:spacing:)`, `HStack`, `ZStack`, `LazyVStack`,
`LazyHStack`, `Group`, `EmptyView()`, `List { ... }`, `List(workspaces, id: \.id) { workspace in ... }`,
`Section("Header") { ... }`, `Section(header: Text("Header"), footer: Text("Footer")) { ... }`,
`Grid { GridRow { ... } }`, constrained
`LazyVGrid(columns: [GridItem(...)])` / `LazyHGrid(rows: [GridItem(...)])`,
`ViewThatFits { ... }`,
`ScrollView { ... }` (use `ScrollView(.horizontal, showsIndicators: false) { HStack { ... } }`
for a horizontal strip, or `ScrollView([.horizontal, .vertical]) { ... }` for
two-axis scrolling; vertical scrolling is automatic), and
`HSplitView { columnA; columnB }` / `VSplitView { top; bottom }`, and
`NavigationStack { ... }`. On Windows/Tauri, split views render as constrained
sidebar-local split panes with independent scrolling; native persisted divider
positions remain follow-up work.
`Group { ... }` renders as a semantic layout-transparent container, while
`EmptyView()` parses as an intentional no-op placeholder. Modifier fan-out from
`Group` to each child remains follow-up work.
`ViewThatFits(in:)` preserves the requested axis as sidebar-local metadata and
renders interpreted alternatives in a semantic container; it does not run
SwiftUI's native fit-measurement pass or hide non-fitting alternatives.
`ScrollView(axes, showsIndicators:)` preserves the interpreted axis and
indicator flag as stable classes plus `data-swift-scroll-*` breadcrumbs, but
does not claim native paging, bounce, scroll-position, or exact platform
scrollbar semantics.
`LazyVGrid` / `LazyHGrid` preserve lazy-grid identity, constrained
`GridItem(.fixed/.flexible/.adaptive)` summaries, spacing, and CSS
`grid-template-*` hints, but they do not claim native SwiftUI virtualization,
exact adaptive packing, or full grid alignment/cell-span semantics.
Grid children also support constrained `.gridCellColumns(...)`,
`.gridColumnAlignment(...)`, and `.gridCellAnchor(...)` metadata lowered to
stable data breadcrumbs and CSS hints; exact native cell placement and
measurement remain follow-up.
Constrained stack `alignment:` tokens on `VStack`, `HStack`, `ZStack`,
`LazyVStack`, and `LazyHStack` lower to stable stack-alignment classes plus
axis-aware CSS alignment hints. Full SwiftUI alignment guides, baseline
alignment, RTL-aware leading/trailing mirroring, and native layout negotiation
remain follow-up work.
`LazyVStack` and `LazyHStack` also preserve constrained `pinnedViews:` tokens
as stable classes and `data-swift-pinned-views` breadcrumbs. Windows/Tauri still
renders those stacks eagerly; native virtualization, sticky section headers or
footers, and pinned scroll physics remain follow-up work.
`Section(header:footer:)` accepts constrained single view expressions for
header and footer slots and renders them through the same safe Swift node path;
arbitrary `@ViewBuilder` header/footer closures, list selection, and native
platform list chrome remain follow-up work.
`List(data, id:)` accepts constrained sequence expressions already supported by
the Swift subset, expands rows through the trailing closure, and preserves the
`id:` token as a `data-swift-list-id` breadcrumb. Selection, edit mode,
SwiftUI identity diffing, and native platform list chrome remain follow-up work.
`ForEach(data, id:)` stamps generated top-level rows with `data-swift-id`
breadcrumbs for constrained scalar/key-path identities; SwiftUI row identity
diffing, move animations, and native reconciliation remain follow-up work.
`TabView { ... }` renders interpreted child pages, and constrained
`.tabViewStyle(.page)` adds stable style/page classes plus a horizontal
scroll-snap hint. Simple `.tabItem { Text(...) }` / `.tabItem { Label(...) }`
chrome is preserved as a sidebar-local tab strip and per-page breadcrumbs.
`TabView(selection:)`, native page indicators, and platform paging physics
remain follow-up work.
Scroll chrome modifiers support constrained `.scrollContentBackground(.hidden)`,
`.scrollIndicators(.hidden, axes:)`, `.scrollClipDisabled()`,
`.scrollTargetBehavior(...)`, `.scrollTargetLayout()`,
`.scrollBounceBehavior(...)`, `.scrollDisabled(...)`, `.scrollPosition(id:)`,
and `.defaultScrollAnchor(...)` classes/data breadcrumbs plus safe CSS hints
where possible; native paging/snap targets, bounce physics, scroll-position
binding, `ScrollViewReader.scrollTo`, and exact platform scrollbar semantics
remain follow-up work.

Content: `Text("...")`, `Label("Title", systemImage: "folder")`,
constrained `Label(title: { Text("Title") }, icon: { Image(systemName: "folder") })`,
`LabeledContent("Title", value: expr)` /
`LabeledContent("Title") { <value views> }`,
`Image(systemName: "folder.fill")` (SF Symbols),
`Image("asset.name")` / `Image(decorative: "asset.name")` for host-provided
sidebar assets,
`AsyncImage(url: URL(string: "https://..."))`,
`Button("Title") { <action> }` / `Button(action:){ <label> }`,
`NavigationLink("Title") { <destination> }` /
`NavigationLink(destination: <view>) { <label> }`,
`NavigationLink(value: expr) { <label> }` with
`.navigationDestination(for: String.self) { value in <destination> }`,
`Link("Title", destination: URL(string: "https://...")!)` /
`Link(destination: URL(string: "https://...")!) { <label> }`,
`ContentUnavailableView("Title", systemImage: "tray", description: Text("..."))`,
`Menu("Title") { <items> }`, `ControlGroup { <controls> }`,
`GroupBox("Title") { <content> }` / `GroupBox(label: { <label> }) { <content> }`,
`DisclosureGroup("Title") { <content> }` /
`DisclosureGroup(isExpanded: $localBool) { <content> } label: { <label> }`,
`TabView { <pages>.tabItem { Text("Tab") } }.tabViewStyle(.page)`,
`ProgressView(value: 0.4)` / `ProgressView()`,
`TextField("Placeholder", text: expr)`, `SecureField("Secret", text: expr)`,
`TextEditor(text: expr)`, `Toggle("Title", isOn: expr)`,
`Slider(value: expr, in: 0...1)`, `Stepper("Title", value: expr, in: 0...10)`,
`DatePicker("Due", selection: expr, displayedComponents: .date)`,
`ColorPicker("Tint", selection: expr)`,
`Picker("Title", selection: expr) { ... }`,
`Gauge(value: 0.7)`, `Spacer()` / `Spacer(minLength:)`, `Divider()`,
`AnyView(<view>)`. `Spacer(minLength:)` emits sidebar-local min-size hints and
data breadcrumbs, but does not claim native SwiftUI layout proposal/expansion
semantics. `TabView` renders authored child pages as a sidebar-local container,
`.tabViewStyle(.page)` turns that container into a horizontal scroll-snap strip,
and simple `.tabItem { Text(...) }` / `.tabItem { Label(...) }` children become
a sidebar-local tab strip plus stable tab/page breadcrumbs; `selection:`
bindings, native paging physics, and platform page indicators remain follow-up. On
Windows/Tauri, direct local `@State` bindings such as `$draftTitle` are editable
inside the open sidebar pane; live cmux/session data bindings remain read-only.
`Picker` supports constrained option `.tag(value)` metadata for text/label-like
options, including options produced by `ForEach`.
`LabeledContent` renders as a read-only inspector row with a label column and a
value/content column; named-label closure overloads and full format-style
semantics remain follow-up work.
`GroupBox` renders as a sidebar-local labeled card/panel with optional rich
label children and `data-swift-group-box-*` breadcrumbs; native/custom
`GroupBoxStyle` rendering remains follow-up.
`DisclosureGroup` renders as a sidebar-local `<details>`/`<summary>` collapsible
section with optional rich label children, expansion breadcrumbs, and direct
local `@State Bool` write-back for `isExpanded:` bindings. Native
`DisclosureGroupStyle`, outline semantics, and list-row integration remain
follow-up.
`Link` renders safe external `http`/`https` destinations as sidebar-local
anchors with `target="_blank"` and `data-swift-link-*` breadcrumbs; unsafe
schemes are visible but disabled instead of becoming clickable.
`ContentUnavailableView` renders compact sidebar-local empty states with optional
SF Symbol fallback icons, rich label/description children, and action buttons;
native platform styling and search-specific convenience behavior remain
follow-up.
`.searchable(text: $localQuery, placement: .sidebar, prompt: "...")` renders a
sidebar-local search field above the modified content, mirrors the prompt and
placement into stable `data-swift-search-*` breadcrumbs, and writes edits back to
direct local `@State String` bindings. Native suggestions, search tokens,
scopes, platform field placement, and non-local/live bindings remain follow-up.
Attach `.onSubmit { ... }` or `.onChange(of:) { ... }` to direct local controls
for simple local `@State` assignments and safe-scoped `cmux(...)` actions.
Attach `.onAppear { ... }` / `.onDisappear { ... }` to rendered nodes for the
same constrained lifecycle handler subset. Attach `.task { ... }` or
`.task(id: value) { ... }` for a constrained mount/id-change hook that runs the
same safe handler subset; this is not full Swift async/await, cancellation,
priority, actor, or structured-concurrency behavior. Attach
`.onHover { hovering in ... }` for browser mouse enter/leave updates using the
same safe local handler subset. Attach `.focusable()` for keyboard focus and
`.focused($localBool)` to mirror browser focus/blur into a direct local `@State`
Bool binding. This is not full `@FocusState` or programmatic SwiftUI focus
ownership.

Shapes: `Rectangle`, `RoundedRectangle(cornerRadius:style:)`,
`UnevenRoundedRectangle`, `Capsule(style:)`, `Circle`, `Ellipse`, and
`ContainerRelativeShape`, plus constrained `Path(roundedRect: CGRect(...))` and
`Path(ellipseIn: CGRect(...))` convenience initializers — fill with
`.fill(color)` / `.foregroundColor`, outline with `.stroke("#hex", lineWidth: 2)`
or constrained `.strokeBorder(color, lineWidth:)`,
arc with `.trim(from:to:)`, size with `.frame`. On Windows/Tauri,
`.trim(from:to:)` is rendered as a CSS partial-fill approximation rather than
SwiftUI's exact vector path geometry, `.strokeBorder(...)` emits stable
`data-swift-shape-stroke*` breadcrumbs and CSS border hints rather than native
insettable-shape stroke placement, `RoundedRectangle(..., style:)` and
`Capsule(style:)` preserve constrained corner-style metadata as
`data-swift-shape-style` breadcrumbs rather than exact native continuous/circular
corner geometry, and `ContainerRelativeShape` is rendered as a
sidebar-local rounded container approximation rather than reading native
container radius. Path convenience initializers render as sidebar-local shape
approximations with `data-swift-path-*` breadcrumbs and CSS size hints; the
imperative `Path { p in ... }` builder, custom `Shape`, and Canvas remain
follow-up work.

Reorder: `Reorderable(data, move: "workspace.reorder") { item in <row> }` (see below).

## Modifiers

Text/typography: `.font(.title2|.headline|.caption|.system(size:design:)...)`,
`.bold()`, `.bold(false)`, `.italic()`, `.italic(false)`,
`.fontWeight(.semibold)`, `.fontDesign(.monospaced)`,
`.fontWidth(.condensed)`, `.dynamicTypeSize(.accessibility2)`, `.monospaced()`, `.monospacedDigit()`, `.lineLimit(2, reservesSpace: true)`, `.truncationMode(.tail)`,
`.multilineTextAlignment(.center)`, `.textCase(.uppercase)`,
`.strikethrough(true, pattern: .dot, color: .red)`,
`.underline(true, pattern: .dash, color: .mint)`, `.tracking(1.5)`,
`.kerning(2)`, and `.baselineOffset(3)`.
Plain `Text(...) + Text(...)` expressions concatenate into one sidebar text
node; preserving native attributed-run modifier boundaries across concatenated
Text values remains follow-up work.
Font width lowers to a CSS `font-stretch` hint, dynamic type size lowers common
size-category tokens to CSS `font-size` hints, `.monospacedDigit()` lowers to a
CSS tabular-number hint, tracking/kerning lower to CSS `letter-spacing`, and
baseline offset lowers to a CSS `vertical-align` hint. `lineLimit(...,
reservesSpace: true)` reserves an approximate CSS line box height when the line
count is numeric; underline/strikethrough pattern and color arguments lower to
CSS text-decoration hints. `truncationMode(.head|.middle|.tail)`,
`multilineTextAlignment(.leading|.center|.trailing)`, and
`textCase(.uppercase|.lowercase)` lower to stable CSS hints. Exact SwiftUI
text-run layout, range/nil line limits, native middle truncation algorithms,
per-decoration styling when underline and strikethrough are combined, variable
font-width availability, host/user Dynamic Type propagation, OpenType feature
availability, and font-engine metrics remain follow-up work.

Color/fill: `.foregroundColor`/`.foregroundStyle`/`.fill`/`.tint` taking a hex
string `"#FF8800"` or a token (`primary`, `secondary`, `tertiary`,
`quaternary`, `quinary`, `accent`, `red`, `blue`, `mint`, `indigo`, `teal`,
`cyan`, `brown`, ...). Hierarchical foreground tokens also expose stable
`cmux-custom-sidebar-swift-foreground-*` classes. `Color("#hex")` /
`Color(red:green:blue:)` values too. Full SwiftUI inherited `ShapeStyle`
resolution and multi-layer foreground styles remain follow-up work.

Layout: `.padding(8)`, `.padding(.horizontal, 6)`,
`.padding([.top, .bottom], 4)`,
`.frame(width:height:minWidth:minHeight:idealWidth:idealHeight:maxWidth:maxHeight:alignment:)`,
`.hidden()`, `.fixedSize()`, `.badge(3)`, `.layoutPriority(1)`,
`.containerRelativeFrame(.horizontal, count:span:spacing:alignment:)`,
`.safeAreaPadding(.horizontal, 8)`, `.contentMargins(.bottom, 6, for: .scrollContent)`,
`.coordinateSpace(name: "board")`,
`.alignmentGuide(.leading) { _ in 12 }`,
`.offset(x:y:)`, `.position(x:y:)`, `.zIndex(1)`,
`.aspectRatio(contentMode:.fit)`, `.scaledToFit()`/`.scaledToFill()`.
Scaled image helpers lower to sidebar-local aspect fit/fill classes; native
SwiftUI image proposal sizing and exact object-fit behavior remain follow-up
work.
Padding supports all-side values, `.top`, `.bottom`, `.leading`, `.trailing`,
`.horizontal`, `.vertical`, `.all`, simple arrays of those tokens, and
`.padding(EdgeInsets(top:leading:bottom:trailing:))` side-specific values.
RTL-aware leading/trailing mirroring and native layout negotiation remain
follow-up work.
Constrained `.safeAreaPadding(...)` and `.contentMargins(..., for:)` forms use
the same edge/length/`EdgeInsets` parser and render sidebar-local padding hints
plus stable metadata breadcrumbs. Native safe-area and scroll-margin layout
negotiation remain follow-up work.
Frame dimensions lower to safe CSS sizing fields, and `maxWidth: .infinity`
still renders as the sidebar fill class. Constrained `alignment:` tokens add
stable frame classes plus CSS text/flex alignment hints; full SwiftUI
proposed-size/ideal-size layout negotiation and native wrapper placement remain
follow-up work. Constrained `.containerRelativeFrame(...)` axis/count/span/
spacing/alignment metadata lowers to stable classes, `data-swift-container-*`
breadcrumbs, and safe CSS sizing hints; native container proposal math and
scroll-target sizing remain follow-up. Constrained `.coordinateSpace(name:)`
preserves the named coordinate-space registration as stable classes and
`data-swift-coordinate-space`; GeometryProxy frame conversion and named-space
lookups remain host-borrowed follow-up. Constrained `.alignmentGuide(...)`
preserves guide metadata and static numeric closure offsets as stable
classes/data breadcrumbs plus a safe CSS margin hint; native `ViewDimensions`
closure evaluation and SwiftUI alignment negotiation remain follow-up.
Constrained `Angle`, `UnitPoint`, `CGPoint`, `CGSize`, and `CGRect` literals are
preserved through the Swift expression evaluator with basic member access, so
geometry values can feed transform/accessibility metadata and string
interpolation. Full CoreGraphics APIs, path construction, affine transforms, and
host `GeometryProxy` values remain follow-up.
`.position(x:y:)` is a CSS-backed placement hint, not full
SwiftUI center-position layout semantics.

Decoration: `.background("#hex")`, `.background(LinearGradient(...))`,
`.background(RadialGradient(...))`, `.background(AngularGradient(...))` **or**
`.background { <view> }`,
`.overlay(alignment:.topTrailing) { <view> }`, `.mask { <view> }`,
`.safeAreaInset(edge:.top) { <view> }`, `.cornerRadius(8)`,
`.clipShape(Circle())`, `.clipShape(Capsule(), style: FillStyle(eoFill:antialiased:))`,
`.clipped()`, `.compositingGroup()`, `.shadow(color:radius:x:y:)`,
`.border(.gray, width:1)`, `.blur(radius:)`, `.opacity(0.6)`, `.hidden()`,
`.brightness`/`.contrast`/`.saturation`/`.grayscale`,
`.hueRotation(.degrees(...))`, `.hueRotation(Angle.radians(...))`,
`.blendMode(.screen|.multiply|...)`,
`.rotationEffect(.degrees(45))`, `.rotationEffect(Angle(degrees: 45))`,
`.scaleEffect(1.2)`,
`.rotation3DEffect(Angle.degrees(30), axis: (x:y:z:), anchor:, perspective:)`,
`.allowsHitTesting(false)`,
`.hoverEffect(.lift, isEnabled: true)`, `.defaultHoverEffect(.highlight)`,
`.visualEffect { content, proxy in content }`,
`.redacted(reason:.placeholder)`, `.privacySensitive()`, and `.unredacted()`.
Blend modes are limited to a safe CSS-backed allow-list. `.compositingGroup()`
creates a CSS `isolation:isolate` boundary, not a native SwiftUI rasterization
pass or exact color-space behavior. The expression evaluator preserves
constrained `Angle`, `UnitPoint`, `CGPoint`, `CGSize`, and `CGRect` literals plus
basic member access, so those values can feed transform/accessibility metadata
and Swift string interpolation. 3D rotation supports scalar/structured angles,
axis tuples, and known or coordinate-equivalent `UnitPoint` anchors as CSS
transform hints; arbitrary UnitPoint placement, `anchorZ`, perspective math, and
native 3D layout remain follow-up.
`.clipShape(_:style:)` preserves constrained `FillStyle(eoFill:antialiased:)`
metadata as stable `data-swift-clip-*` breadcrumbs; exact native clipping paths,
fill-rule geometry, and rasterization/antialias behavior remain follow-up.
`.safeAreaPadding(...)` and `.contentMargins(..., for:)` preserve constrained
edge/length/`EdgeInsets` metadata as stable `data-swift-safe-area-padding-*` and
`data-swift-content-margins-*` breadcrumbs with sidebar-local padding hints;
native safe-area environment negotiation, scroll-content margin behavior, and
platform inset semantics remain follow-up.
`.visualEffect` preserves authored modifier metadata as stable classes/data
breadcrumbs only; GeometryProxy-driven transforms remain follow-up.
Redaction and privacy modifiers render as sidebar-local placeholder/privacy
metadata with stable `data-swift-redacted`, `data-swift-redaction-reason`,
`data-swift-privacy-sensitive`, and `data-swift-unredacted` breadcrumbs. Native
inherited redaction environments, OS privacy lock-state propagation, and exact
SwiftUI placeholder drawing remain follow-up.

Images/SF Symbols: `.resizable()`,
`.resizable(capInsets: EdgeInsets(top:leading:bottom:trailing:), resizingMode: .stretch|.tile)`,
`.renderingMode(.template|.original)`,
`.interpolation(.none|.low|.medium|.high)`, `.antialiased(false)`,
`.flipsForRightToLeftLayoutDirection(true)`,
`.imageScale(.large)`, `.symbolRenderingMode(.hierarchical)`, `.symbolVariant(.fill)`.
`Image(systemName:)` renders a constrained text-glyph fallback with stable
`data-swift-system-image` and `data-swift-system-image-glyph` breadcrumbs; this
does not claim native SF Symbol vector paths, weights, variable values, or exact
symbol rendering.
`Image("name")` and `Image(decorative:)` resolve through the
`extension.sidebar.snapshot` `assets` map when the host provides one. Asset URLs
must be host-minted safe URLs (`https?://...` or `cmux-sidebar-asset://...`);
missing or unsafe assets render a visible placeholder instead of loading local
files. On Windows/Tauri, place local image files beside the sidebar in a sibling
`<sidebar-name>.assets/` directory; for example
`sidebars/ops.assets/logo.png` is available as `Image("logo")` and
`Image("logo.png")`. `Image(decorative:)` renders with empty alt text and
forced `aria-hidden`, so authored accessibility labels do not leak onto
decorative assets.
Resizable cap insets and resizing mode are preserved as safe CSS custom
properties plus stable classes for renderer parity; Windows/Tauri does not yet
perform native SwiftUI nine-slice or tiled image drawing.
`AsyncImage(url:)` accepts `http`/`https` URLs and renders a bounded lazy
remote image; `file:`, `data:`, and app-internal URLs are rejected to a visible
placeholder. Rendered nodes expose constrained `data-swift-async-image-phase`
breadcrumbs (`success` for accepted URLs, `failure` for rejected URLs) plus the
accepted safe URL as `data-swift-async-image-url`. Constrained named
`AsyncImage(url:content:placeholder:)` closures can render interpreted success
content plus placeholder breadcrumbs, including common `image in
image.resizable()` parameter bodies. Both named closure arguments and SwiftUI's
`AsyncImage(url:) { image in ... } placeholder: { ... }` spelling are supported;
native phase values and true load lifecycle remain follow-up work.

Interaction/semantics: `.onTapGesture { <action> }` (any view tappable),
`.onTapGesture(count: 2) { <action> }` (double-click/tap affordance),
`.onLongPressGesture { <action> }` (press-and-hold affordance),
`.labelsHidden()`,
`.contextMenu { <buttons> }`, `.refreshable { <action> }`,
`.swipeActions(edge: .leading/.trailing, allowsFullSwipe:) { <buttons> }`,
`.dropDestination(for: String.self) { items, location in cmux(...) }`,
`.onGeometryChange(for: CGSize.self) { proxy in proxy.size }`,
`.help("tip")`, `.disabled(cond)`,
`.accessibilityLabel("...")`, `.accessibilityValue("...")`,
`.accessibilityHint("...")`, `.accessibilityAddTraits(...)`,
`.accessibilityElement(children: .combine/.contain/.ignore)`,
`.accessibilityAction(named: Text("Refresh")) { cmux(...) }`,
`.accessibilityActivationPoint(CGPoint(x:y:))`,
`.accessibilityRepresentation { Label("Readable", systemImage: "text.magnifyingglass") }`,
and `.accessibilitySortPriority(...)`. The Windows/Tauri renderer maps the direct
ARIA equivalents where possible and exposes the richer SwiftUI-only semantics as
`data-swift-accessibility-*` breadcrumbs; full VoiceOver rotor/activation-point
semantics and native custom action menus remain follow-up work. Constrained
`.accessibilityAction` closures that contain a safe `cmux(...)` action are
keyboard-invokable with Enter/Space and expose stable action breadcrumbs.
Constrained `.accessibilityActivationPoint(...)` values are metadata-only
breadcrumbs; the browser renderer does not move native assistive activation
geometry.
Constrained `.accessibilityRepresentation { ... }` content is preserved as a
hidden, `aria-hidden` metadata mirror with stable representation data
breadcrumbs; it does not replace the node in native assistive technology.
`.onGeometryChange` preserves the requested value type and transform-closure
presence as stable classes/data breadcrumbs only; host-supplied geometry,
transform evaluation, and action execution remain follow-up work.

Built-in control styles: `.controlSize(.mini|.small|.regular|.large)`,
`.buttonStyle(.plain|.bordered|.borderedProminent)`,
`.buttonBorderShape(.capsule|.roundedRectangle|.circle)`,
`.labelStyle(.titleOnly|.iconOnly)`, `.labelsHidden()`, `.menuStyle(.button)`,
`.controlGroupStyle(.compactMenu|.menu|.palette|.navigation)`,
`.pickerStyle(.segmented|.menu)`, `.toggleStyle(.button|.switch)`, and
`.textFieldStyle(.roundedBorder|.plain)`. Custom style structs are not
interpreted.

The decoration/action/metadata modifiers that take a trailing `{ <view> }`
(`.overlay`, `.background`, `.mask`, `.safeAreaInset`, `.contextMenu`,
`.swipeActions`, `.accessibilityRepresentation`)
accept **any** nested view, so you can compose badges, rings, status dots, and
action trays or preserve a readable accessibility mirror. `.refreshable { cmux(...) }`
renders a sidebar-local Refresh
button that runs the authored safe action. These are command affordances, not
native pull-to-refresh physics, platform swipe gestures, or SwiftUI assistive
representation substitution.
`.dropDestination(for:) { cmux(...) }` marks the node as a browser drop target
and invokes the authored safe action on drop; typed `Transferable` payloads,
dropped item/location binding, and native drag/drop highlighting remain
follow-up work.

Style effects are intentionally CSS-backed: `LinearGradient(colors:startPoint:endPoint:)`,
`RadialGradient(colors:...)`, `AngularGradient(colors:...)`, and
`Color.<token>.gradient` are accepted for `.background`, `.foregroundStyle`,
`.fill`, and `.tint`. Color stops are mapped through cmux's safe token palette;
custom color spaces and gradient stop locations remain follow-up work.
`.background(.regularMaterial)` and related Material tokens render as safe
translucent CSS backgrounds with blur/saturation and stable material classes;
native SwiftUI vibrancy/material blending and host-window sampling remain
follow-up work.

Animation modifiers are metadata-only today: `.animation(...)`, `.transition(...)`,
`.contentTransition(...)`, `.symbolEffect(...)`, and `.symbolEffectsRemoved()` are
parsed and exposed as safe classes plus `data-swift-*` breadcrumbs; removal clears
previous symbol-effect breadcrumbs in the same modifier chain. They do not animate
state changes until the broader re-walk/diff animation engine exists.

## Language

`let` bindings; user `func` helpers (value helpers and view helpers returning
`some View`, explicit `return` supported); `for i in 0..<n` / `1...n` /
`for x in array`; `ForEach(array) { item in ... }`,
`ForEach(array.indices) { i in }`, and
`ForEach(Array(array.enumerated()), id: \.offset) { i, item in }`; `if/else`;
ternary `cond ? a : b` (works in modifiers and interpolation); string
interpolation `"\(expr)"`; arithmetic `+ - * / %` (safe on `/ 0`); comparisons;
`&& || !` (short-circuiting); ranges; array/dictionary literals; member access
(`obj.field`, `array.count`/`.first`/`.last`/`.indices`, `string.count`);
subscript `array[i]`, `obj["key"]`.

Array methods: `.filter`, `.map`, `.flatMap`, `.reduce`, `.sorted { $0 > $1 }`,
`.first`, `.contains`, `.count`, `.reversed`, `.prefix(n)`, `.suffix(n)`,
`.dropFirst(n)`, `.dropLast(n)`, `.enumerated()`, `.indices`. String methods:
`.hasPrefix`, `.hasSuffix`, `.contains`, `.uppercased()`, `.lowercased()`,
`.split(separator:)`. Numbers: `.formatted(.currency(code:"USD"))` /
`.formatted(.percent)` / `.formatted(.notation(.compactName))` /
`.formatted(.byteCount(style: .file))` /
`Measurement(value:unit:).formatted(.measurement(width: ...))` /
`Date(timeIntervalSince1970:).formatted(.dateTime...)` /
`Text(date, style: .date/.time)` /
`Text(timerInterval: start...end)` static timer snapshots /
`["A", "B"].formatted(.list(type: .and))`. Builtins:
`min`, `max`, `abs`, `Int(...)`, `Double(...)`, `String(...)`, and
constrained `String(format:)` for common `%d`/`%f`/`%@`/`%s` forms. Full
Foundation locale formatting, broad printf specifiers, unit conversion, broad
unit catalogs, relative/live date styles, self-updating timer runloop behavior,
rich list-style composition, and full
`FormatStyle` composition remain follow-up.

## Actions (run real cmux commands on tap)

A button or `.onTapGesture` body calls `cmux("<method>", param: value)`. On tap
it runs that cmux command through the same dispatcher as the `cmux` CLI:

    Button(action: { cmux("workspace.select", workspace_id: w.id) }) { ... }
    ...onTapGesture { cmux("surface.focus", surface_id: t.id) }
    Button("Half") { cmux("workspace.set_progress", value: 0.5, hidden: false) }

Swift action params preserve basic JSON-compatible value types: strings,
numbers, booleans, `nil`/`null`, arrays, ternary results, and resolved live data
fields such as `workspaceCount`, `w.id`, and `workspaces[0].selected`.

Use real method and parameter names. Common safe-default ones:
`workspace.select` (`workspace_id` or `workspace_ref`), `surface.focus`
(`surface_id` or `surface_ref`), `sidebar.reload` (`name` optional),
`sidebar.select`/`sidebar.open` (`name`), `workspace.set_progress`
(`value`), `workspace.set_status` (`key`, `value`), `workspace.report_meta`
(`key`, `value`), `workspace.report_meta_block` (`key`, `markdown`), and
`workspace.log` (`message`). The desktop capabilities payload advertises this
authored-action schema under `custom_sidebar_actions.schema`.

## Drag-and-drop reordering (persisted)

Drag-and-drop is achieved with `Reorderable`. This is the supported way to make
a list draggable, do not reach for `List`/`.onMove`/`.draggable` directly. Wrap
rows in `Reorderable`; the rows become draggable and dropping one onto another
runs the `move` command, which both reorders and persists (cmux remembers
workspace order):

    Reorderable(workspaces, move: "workspace.reorder") { w in
        Button(action: { cmux("workspace.select", workspace_id: w.id) }) {
            HStack { Text(w.title); Spacer() }.padding(6)
        }
    }

The dropped item's id and target index are sent as `workspace_id` and `index`.

## Two-column (Finder-style) example

    HSplitView {
        VStack(alignment: .leading) {
            for i in 0..<workspaces.count {
                Button(action: { cmux("workspace.select", workspace_id: workspaces[i].id) }) {
                    HStack { Image(systemName: "folder.fill"); Text(workspaces[i].title); Spacer() }.padding(4)
                }
            }
        }
        VStack(alignment: .leading) {
            for i in 0..<workspaces.count {
                if workspaces[i].selected {
                    for j in 0..<workspaces[i].tabs.count {
                        Button(action: { cmux("surface.focus", surface_id: workspaces[i].tabs[j].id) }) {
                            HStack { Image(systemName: "doc.text"); Text(workspaces[i].tabs[j].title); Spacer() }.padding(4)
                        }
                    }
                }
            }
        }
    }

## Not yet supported

The interpreter is a growing subset. `.overlay`/`.background`/`.mask`/
`.contextMenu` with arbitrary nested views, `Menu`, `List`/`Section`/grids,
shape `.stroke`/`.trim`, and user `func` helpers are all supported now.

Still missing: full SwiftUI state lifecycle semantics beyond simple local
`@State var` declarations with direct `$name` bindings, constrained
`ForEach($localCollection)` element-field bindings, and constrained event,
submit, and change handlers;
mutation of live cmux/session data through two-way controls; arbitrary Swift
logic inside handlers; true Swift `Date`/`Color` value semantics; true `.onChange` diffing across re-walks; `switch`; custom `struct`/`View` definitions;
custom gradient stops/materials; full `.id(_:)` identity/replacement behavior;
navigation/presentation (`NavigationSplitView`,
route/path bindings, `navigationDestination` beyond the constrained value-link subset, full native `dismiss`, and full Identifiable
item presentations);
native asset catalogs beyond host-provided sidebar asset URLs; `AsyncImage`
content/placeholder/phase closures. Workspace
data (git branch/dirty, ports, PR, unread, remote, latest agent/prompt messages)
is live; data cmux doesn't track (custom domain collections) won't appear.

If your sidebar needs a missing feature, write it the natural Swift way anyway —
unsupported syntax is skipped (and even deeply nested or pathological source is
rendered best-effort, never crashes) — and ask for the feature.

## Performance and lazy loading

The sidebar re-evaluates roughly once a second (so clocks and data stay live),
and it renders rows eagerly. Keep each render cheap and the list bounded:

- Cap long lists. Show what fits and slice the rest: `for w in workspaces.prefix(20) { ... }`
  or `ForEach(items.prefix(50)) { ... }`. Do not render hundreds of rows.
- Filter/sort to what matters before rendering (`workspaces.filter { ... }`,
  `.sorted()`) rather than rendering everything and hiding most of it.
- Only render detail for the selected item. In a two-column layout, build the
  right column from the selected workspace's tabs, not every workspace's tabs.
- Prefer one focused sidebar over a giant catch-all; deep nesting and huge
  trees cost the most per tick.

## Tips

- Prefer `ForEach`/`Reorderable` over index loops where you can.
- Errors show inline in the sidebar with the failing location; fix and save.
- Keep modifier arguments simple literals or tokens.
- The JSON form is good for static layouts; use Swift for anything dynamic.
