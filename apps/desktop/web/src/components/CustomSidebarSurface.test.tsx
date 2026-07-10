import { describe, expect, mock, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import type { SessionWorkspaceSnapshot } from "@cmux/core-types";

let currentWorkspaces: readonly SessionWorkspaceSnapshot[] = [];
let currentSelectedWorkspaceIndex = 0;

mock.module("../hooks/useSession", () => ({
  useSession: () => ({
    workspaces: currentWorkspaces,
    selectedWorkspaceIndex: currentSelectedWorkspaceIndex,
    selectWorkspace: () => {},
    selectWorkspaceSurface: () => {},
  }),
}));

const {
  CustomSidebarSurface,
  customSidebarAssetMapFromSnapshot,
  customSidebarEventsContextFromSnapshot,
  customSidebarActionErrorMessage,
  customSidebarReloadMatches,
  customSidebarSourceKind,
  customSidebarSourceName,
  customSidebarSwiftEventHandlerMatches,
  emptyCustomSidebarEventsContext,
  interpolateCustomSidebarTemplate,
  nextCustomSidebarEventsContext,
  parseCustomSidebarJson,
  parseCustomSidebarSwift,
  resolveCustomSidebarActionParams,
} = await import("./CustomSidebarSurface");
const { NativeBridgeError } = await import("../tauri-bridge");

function workspace(
  overrides: Partial<SessionWorkspaceSnapshot> = {},
): SessionWorkspaceSnapshot {
  return {
    process_title: "shell",
    current_directory: "C:\\repo",
    layout: {
      type: "pane",
      pane: {
        panel_ids: ["surface-1"],
      },
    },
    ...overrides,
  };
}

function workspacePreview(
  overrides: Partial<{
    id: string;
    index: number;
    title: string;
    selected: boolean;
    unreadCount: number;
    ports: number[];
    progress: number;
  }> = {},
) {
  return {
    id: overrides.id ?? `workspace-${overrides.index ?? 0}`,
    index: overrides.index ?? 0,
    title: overrides.title ?? "Workspace",
    tabs: [],
    tabCount: 0,
    unreadCount: overrides.unreadCount ?? 0,
    ports: overrides.ports ?? [],
    dirty: false,
    progress: overrides.progress,
    statusCount: 0,
    metadataCount: 0,
    logCount: 0,
    selected: overrides.selected ?? false,
  };
}

describe("customSidebarSourceName", () => {
  test("uses the basename of a Windows or POSIX sidebar path", () => {
    expect(customSidebarSourceName("C:\\Users\\User\\.config\\cmux\\sidebars\\ops.swift")).toBe(
      "ops.swift",
    );
    expect(customSidebarSourceName("/home/user/.config/cmux/sidebars/prs.json")).toBe(
      "prs.json",
    );
  });

  test("falls back when no source path is stored", () => {
    expect(customSidebarSourceName(" ")).toBe("Unsaved custom sidebar");
  });
});

describe("custom sidebar JSON helpers", () => {
  test("detects JSON and Swift source kinds", () => {
    expect(customSidebarSourceKind("C:\\sidebars\\ops.json")).toBe("json");
    expect(customSidebarSourceKind("/sidebars/ops.swift")).toBe("swift");
    expect(customSidebarSourceKind("/sidebars/ops.txt")).toBe("custom");
  });

  test("parses object documents and rejects non-object JSON", () => {
    expect(parseCustomSidebarJson('{"title":"Ops","blocks":[]}')).toEqual({
      title: "Ops",
      blocks: [],
    });
    expect(() => parseCustomSidebarJson("[]")).toThrow(
      "Custom sidebar JSON must be an object.",
    );
    expect(() => parseCustomSidebarJson('{"blocks":{}}')).toThrow(
      "Custom sidebar JSON field 'blocks' must be an array.",
    );
  });

  test("interpolates known session placeholders and leaves unknown tokens", () => {
    expect(
      interpolateCustomSidebarTemplate(
        "{selectedTitle} / {workspaceCount} / {missing}",
        {
          sourceName: "ops.json",
          workspaceCount: 2,
          selectedTitle: "Review",
          selectedId: "w2",
          unreadTotal: 1,
          portTotal: 3,
        },
      ),
    ).toBe("Review / 2 / {missing}");
  });

  test("resolves nested action params with workspace and tab placeholders", () => {
    expect(
      resolveCustomSidebarActionParams(
        {
          workspace_id: "{workspace.id}",
          surface_id: "{tab.id}",
          nested: ["{selectedTitle}", 7],
        },
        {
          sourceName: "ops.json",
          workspaceCount: 1,
          selectedTitle: "Ops",
          selectedId: "w1",
          unreadTotal: 0,
          portTotal: 0,
          workspace: {
            id: "w1",
            index: 0,
            title: "Ops",
            tabs: [],
            tabCount: 0,
            unreadCount: 0,
            ports: [],
            dirty: false,
            statusCount: 0,
            metadataCount: 0,
            logCount: 0,
            selected: true,
          },
          tab: {
            id: "surface-1",
            title: "Shell",
            dirty: false,
            ports: [],
            focused: true,
          },
        },
      ),
    ).toEqual({
      workspace_id: "w1",
      surface_id: "surface-1",
      nested: ["Ops", 7],
    });
  });

  test("formats custom sidebar action policy failures for inline UI", () => {
    expect(
      customSidebarActionErrorMessage(
        new NativeBridgeError(
          "Custom sidebar action 'browser.eval' is outside the safe capability scope",
          "custom_sidebar_capability_denied",
          {
            manifest: {
              denied_requested_methods: ["browser.eval"],
            },
          },
        ),
      ),
    ).toBe(
      "Custom sidebar action 'browser.eval' is outside the safe capability scope. Manifest requested: browser.eval. These are not granted by the safe default policy.",
    );
    expect(
      customSidebarActionErrorMessage(
        new NativeBridgeError(
          "Custom sidebar action 'workspace.select' has invalid params",
          "custom_sidebar_action_schema_invalid",
          {
            accepted_keys: ["workspace_id", "id", "workspace_ref", "ref"],
            expected: "non-empty workspace_id/id or workspace_ref/ref string",
            field: "workspace",
          },
        ),
      ),
    ).toBe(
      "Custom sidebar action params need workspace: expected non-empty workspace_id/id or workspace_ref/ref string. Accepted keys: workspace_id, id, workspace_ref, ref.",
    );
    expect(customSidebarActionErrorMessage(new Error("boom"))).toBe(
      "Custom sidebar action failed: boom",
    );
  });

  test("matches reload events by all flag, source name, paths, and sidebar entries", () => {
    const sourcePath = "C:\\Users\\User\\.config\\cmux\\sidebars\\ops.json";

    expect(customSidebarReloadMatches({ all: true }, sourcePath)).toBe(true);
    expect(customSidebarReloadMatches({ name: "ops" }, sourcePath)).toBe(true);
    expect(
      customSidebarReloadMatches(
        { paths: ["c:/users/user/.config/cmux/sidebars/ops.json"] },
        sourcePath,
      ),
    ).toBe(true);
    expect(
      customSidebarReloadMatches(
        { sidebars: [{ name: "ops", path: "C:\\other\\ops.json" }] },
        sourcePath,
      ),
    ).toBe(true);
    expect(customSidebarReloadMatches({ name: "review" }, sourcePath)).toBe(false);
    expect(customSidebarReloadMatches({ all: true }, undefined)).toBe(false);
  });

  test("builds custom sidebar event context from snapshots and live events", () => {
    const seeded = customSidebarEventsContextFromSnapshot({
      boot_id: "boot-1",
      latest: {
        boot_id: "boot-1",
        category: "workspace",
        name: "workspace.selected",
        seq: 7,
      },
      latest_seq: 7,
      next_seq: 8,
      oldest_seq: 3,
      recent: [
        { category: "session", name: "session.changed", seq: 6 },
        { category: "workspace", name: "workspace.selected", seq: 7 },
      ],
      category_counts: { session: 1, workspace: 1, bad: "nope" },
      name_counts: { "session.changed": 1, "workspace.selected": 1 },
      retained_count: 2,
    });

    expect(seeded.latest?.name).toBe("workspace.selected");
    expect(seeded.recent).toHaveLength(2);
    expect(seeded.category_counts).toEqual({ session: 1, workspace: 1 });
    expect(seeded.latest_seq).toBe(7);
    expect(seeded.next_seq).toBe(8);

    let live = emptyCustomSidebarEventsContext();
    for (let index = 1; index <= 55; index += 1) {
      live = nextCustomSidebarEventsContext(live, {
        category: index % 2 === 0 ? "sidebar" : "workspace",
        name: "workspace.selected",
        seq: index,
      });
    }

    expect(live.latest_seq).toBe(55);
    expect(live.next_seq).toBe(56);
    expect(live.retained_count).toBe(55);
    expect(live.recent).toHaveLength(50);
    expect(live.recent[0]?.seq).toBe(6);
    expect(live.name_counts["workspace.selected"]).toBe(55);
    expect(live.category_counts.workspace).toBe(28);
    expect(live.category_counts.sidebar).toBe(27);
  });

  test("normalizes custom sidebar image assets from snapshot payloads", () => {
    expect(
      customSidebarAssetMapFromSnapshot({
        logo: "https://example.com/logo.png",
        local: "cmux-sidebar-asset://ops/local.png",
        unsafe: "file:///etc/passwd",
      }),
    ).toEqual({
      logo: "https://example.com/logo.png",
      local: "cmux-sidebar-asset://ops/local.png",
    });

    expect(
      customSidebarAssetMapFromSnapshot([
        { name: "avatar", src: "https://example.com/avatar.png" },
        { name: "badge", href: "cmux-sidebar-asset://ops/badge.svg" },
        { name: "secret", url: "data:image/svg+xml,boom" },
      ]),
    ).toEqual({
      avatar: "https://example.com/avatar.png",
      badge: "cmux-sidebar-asset://ops/badge.svg",
    });
  });

  test("parses a supported SwiftUI subset into render nodes with cmux actions", () => {
    const document = parseCustomSidebarSwift(
      `
      VStack(spacing: 8) {
        Text("Workspaces: \\(workspaceCount)")
        Divider()
        ForEach(workspaces) { w in
          Button(action: { cmux("workspace.select", workspace_id: w.id) }) {
            HStack {
              Text(w.selected ? "●" : "○")
              Text(w.title)
              Spacer(minLength: 24)
            }
          }
        }
      }
      `,
      {
        sourceName: "mine.swift",
        workspaceCount: 2,
        selectedTitle: "Ops",
        selectedId: "w1",
        unreadTotal: 0,
        portTotal: 0,
      },
      [
        {
          id: "w1",
          index: 0,
          title: "Ops",
          tabs: [],
          tabCount: 0,
          unreadCount: 0,
          ports: [],
          dirty: false,
          statusCount: 0,
          metadataCount: 0,
          logCount: 0,
          selected: true,
        },
        {
          id: "w2",
          index: 1,
          title: "API",
          tabs: [],
          tabCount: 0,
          unreadCount: 0,
          ports: [],
          dirty: false,
          statusCount: 0,
          metadataCount: 0,
          logCount: 0,
          selected: false,
        },
      ],
    );

    expect(document.root.kind).toBe("vstack");
    expect(document.warnings).toEqual([]);
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]).toEqual({ kind: "text", text: "Workspaces: 2" });
    expect(root?.children[2]?.kind).toBe("button");
    if (root?.children[2]?.kind === "button") {
      expect(root.children[2].action).toEqual({
        method: "workspace.select",
        params: { workspace_id: "w1" },
      });
      expect(root.children[2].children[0]).toMatchObject({
        kind: "hstack",
        children: [
          { kind: "text", text: "●" },
          { kind: "text", text: "Ops" },
          { kind: "spacer", minLength: 24 },
        ],
      });
    }
  });

  test("preserves typed Swift cmux action params", () => {
    const document = parseCustomSidebarSwift(
      `
      VStack {
        Button("Report") {
          cmux(
            "workspace.set_progress",
            value: 0.42,
            enabled: true,
            selected: workspaces[0].selected,
            count: workspaceCount,
            title: selectedTitle,
            mode: .compact,
            fallback: nil,
            tags: ["alpha", selectedTitle, 3],
            priority: selectedId == "w1" ? 7 : 2
          )
        }
      }
      `,
      {
        sourceName: "mine.swift",
        workspaceCount: 2,
        selectedTitle: "Ops",
        selectedId: "w1",
        unreadTotal: 0,
        portTotal: 0,
      },
      [
        workspacePreview({
          id: "w1",
          index: 0,
          title: "Ops",
          selected: true,
        }),
      ],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]?.kind).toBe("button");
    if (root?.children[0]?.kind === "button") {
      expect(root.children[0].action).toEqual({
        method: "workspace.set_progress",
        params: {
          value: 0.42,
          enabled: true,
          selected: true,
          count: 2,
          title: "Ops",
          mode: "compact",
          fallback: null,
          tags: ["alpha", "Ops", 3],
          priority: 7,
        },
      });
    }
  });

  test("captures onTapGesture cmux actions as tappable view wrappers", () => {
    const document = parseCustomSidebarSwift(
      `
      VStack {
        Text("Open Ops")
          .font(.headline)
          .onTapGesture {
            cmux("workspace.select", workspace_id: selectedId)
          }
      }
      `,
      {
        sourceName: "mine.swift",
        workspaceCount: 1,
        selectedTitle: "Ops",
        selectedId: "w1",
        unreadTotal: 0,
        portTotal: 0,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]?.kind).toBe("button");
    if (root?.children[0]?.kind === "button") {
      expect(root.children[0].modifiers).toEqual([
        { name: "buttonStyle", value: "plain" },
      ]);
      expect(root.children[0].action).toEqual({
        method: "workspace.select",
        params: { workspace_id: "w1" },
      });
      expect(root.children[0].children[0]).toEqual({
        kind: "text",
        text: "Open Ops",
        modifiers: [{ name: "font", value: "headline" }],
      });
    }
  });

  test("captures tap counts and long press gestures as safe action wrappers", () => {
    const document = parseCustomSidebarSwift(
      `
      VStack {
        Text("Double")
          .onTapGesture(count: 2) {
            cmux("workspace.select", workspace_id: selectedId)
          }
        Text("Hold")
          .onLongPressGesture {
            cmux("sidebar.reload", name: sourceName)
          }
      }
      `,
      {
        sourceName: "mine.swift",
        workspaceCount: 1,
        selectedTitle: "Ops",
        selectedId: "w1",
        unreadTotal: 0,
        portTotal: 0,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]).toMatchObject({
      kind: "button",
      actionTrigger: "tap",
      tapCount: 2,
      action: {
        method: "workspace.select",
        params: { workspace_id: "w1" },
      },
    });
    expect(root?.children[1]).toMatchObject({
      kind: "button",
      actionTrigger: "longPress",
      action: {
        method: "sidebar.reload",
        params: { name: "mine.swift" },
      },
    });
  });

  test("parses Toggle live-data and constant bindings", () => {
    const document = parseCustomSidebarSwift(
      `
      VStack {
        Toggle("Selected", isOn: workspaces[0].selected)
        Toggle(isOn: .constant(false)) {
          Text("Manual")
        }
      }
      `,
      {
        sourceName: "mine.swift",
        workspaceCount: 1,
        selectedTitle: "Ops",
        selectedId: "w1",
        unreadTotal: 0,
        portTotal: 0,
      },
      [
        workspacePreview({
          id: "w1",
          index: 0,
          title: "Ops",
          selected: true,
        }),
      ],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]).toEqual({
      kind: "toggle",
      text: "Selected",
      isOn: true,
      children: [],
    });
    expect(root?.children[1]).toEqual({
      kind: "toggle",
      isOn: false,
      children: [{ kind: "text", text: "Manual" }],
    });
  });

  test("parses TextField and Slider live-data bindings", () => {
    const document = parseCustomSidebarSwift(
      `
      VStack {
        TextField("Selected workspace", text: $selectedTitle)
        Slider(value: $workspaceCount, in: 0...4) {
          Text("Workspace capacity")
        }
      }
      `,
      {
        sourceName: "mine.swift",
        workspaceCount: 1,
        selectedTitle: "Ops",
        selectedId: "w1",
        unreadTotal: 0,
        portTotal: 0,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]).toEqual({
      kind: "textField",
      placeholder: "Selected workspace",
      text: "Ops",
      children: [],
    });
    expect(root?.children[1]).toEqual({
      kind: "slider",
      value: 1,
      lowerBound: 0,
      upperBound: 4,
      children: [{ kind: "text", text: "Workspace capacity" }],
    });
  });

  test("parses Picker selection bindings and option views", () => {
    const document = parseCustomSidebarSwift(
      `
      Picker("Workspace", selection: $selectedTitle) {
        ForEach(workspaces) { workspace in
          Text(workspace.title)
        }
      }
      `,
      {
        sourceName: "mine.swift",
        workspaceCount: 2,
        selectedTitle: "Ops",
        selectedId: "w1",
        unreadTotal: 0,
        portTotal: 0,
      },
      [
        workspacePreview({ id: "w1", index: 0, title: "Ops", selected: true }),
        workspacePreview({ id: "w2", index: 1, title: "API" }),
      ],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root).toEqual({
      kind: "picker",
      title: "Workspace",
      selection: "Ops",
      selectedValue: "Ops",
      options: [
        { label: "Ops", value: "Ops", encodedValue: '"Ops"' },
        { label: "API", value: "API", encodedValue: '"API"' },
      ],
      children: [
        { kind: "text", text: "Ops" },
        { kind: "text", text: "API" },
      ],
    });
  });

  test("parses Picker option tags as selection values", () => {
    const document = parseCustomSidebarSwift(
      `
      @State var lane = "api"
      Picker("Lane", selection: $lane) {
        Text("Operations").tag("ops")
        Text("API").tag("api")
        Text("Docs").tag("docs")
      }
      `,
      {
        sourceName: "picker-tags.swift",
        workspaceCount: 0,
        selectedTitle: "",
        selectedId: "",
        unreadTotal: 0,
        portTotal: 0,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root).toMatchObject({
      kind: "picker",
      title: "Lane",
      selection: "API",
      selectedValue: "api",
      stateBindingKey: "lane",
      options: [
        { label: "Operations", value: "ops", encodedValue: '"ops"' },
        { label: "API", value: "api", encodedValue: '"api"' },
        { label: "Docs", value: "docs", encodedValue: '"docs"' },
      ],
    });
  });

  test("parses local @State bindings for editable Swift controls", () => {
    const document = parseCustomSidebarSwift(
      `
      @State private var draftTitle = "Draft"
      @State var enabled = false
      @State var capacity = 2
      @State var lane = "Ops"
      @State var secret = "hunter2"
      @State var notes = "Line one"
      @State var count = 1
      @State var status = "idle"
      @State var due = "2026-07-09"
      @State var tint = "#2dd4bf"

      VStack {
        Text(draftTitle)
        TextField("Draft title", text: $draftTitle)
        SecureField("Token", text: $secret)
        TextEditor(text: $notes)
        Toggle("Enabled", isOn: $enabled)
        Slider(value: $capacity, in: 0...4)
        Stepper("Count", value: $count, in: 0...3)
        DatePicker("Due", selection: $due, displayedComponents: .date)
        ColorPicker("Tint", selection: $tint)
        Picker("Lane", selection: $lane) {
          Text("Ops")
          Text("API")
        }
        Text("Lifecycle")
          .onAppear {
            status = "appeared"
          }
          .onDisappear {
            status = "gone"
          }
      }
      `,
      {
        sourceName: "state.swift",
        workspaceCount: 0,
        selectedTitle: "",
        selectedId: "",
        unreadTotal: 0,
        portTotal: 0,
      },
      [],
      {
        draftTitle: "Edited",
        enabled: true,
        capacity: 3,
        lane: "API",
        secret: "changed",
        notes: "Edited note",
        count: 2,
        due: "2026-07-10",
        tint: "#14b8a6",
      },
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]).toEqual({ kind: "text", text: "Edited" });
    expect(root?.children[1]).toMatchObject({
      kind: "textField",
      text: "Edited",
      stateBindingKey: "draftTitle",
    });
    expect(root?.children[2]).toMatchObject({
      kind: "textField",
      text: "changed",
      secure: true,
      stateBindingKey: "secret",
    });
    expect(root?.children[3]).toMatchObject({
      kind: "textField",
      text: "Edited note",
      multiline: true,
      stateBindingKey: "notes",
    });
    expect(root?.children[4]).toMatchObject({
      kind: "toggle",
      isOn: true,
      stateBindingKey: "enabled",
    });
    expect(root?.children[5]).toMatchObject({
      kind: "slider",
      value: 3,
      stateBindingKey: "capacity",
    });
    expect(root?.children[6]).toMatchObject({
      kind: "stepper",
      title: "Count",
      value: 2,
      lowerBound: 0,
      upperBound: 3,
      step: 1,
      stateBindingKey: "count",
    });
    expect(root?.children[7]).toMatchObject({
      kind: "datePicker",
      title: "Due",
      value: "2026-07-10",
      displayedComponents: "date",
      stateBindingKey: "due",
    });
    expect(root?.children[8]).toMatchObject({
      kind: "colorPicker",
      title: "Tint",
      value: "#14b8a6",
      stateBindingKey: "tint",
    });
    expect(root?.children[9]).toMatchObject({
      kind: "picker",
      selection: "API",
      stateBindingKey: "lane",
    });
  });

  test("parses Swift Form as a sidebar-local form container", () => {
    const document = parseCustomSidebarSwift(
      `
      @State var enabled = true
      @State var title = "Ops"

      Form {
        Section("Settings") {
          TextField("Title", text: $title)
          Toggle("Enabled", isOn: $enabled)
        }
      }
      `,
      {
        sourceName: "form.swift",
        workspaceCount: 0,
        selectedTitle: "",
        selectedId: "",
        unreadTotal: 0,
        portTotal: 0,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root).toMatchObject({
      kind: "list",
      form: true,
      children: [
        {
          kind: "section",
          title: "Settings",
          children: [
            { kind: "textField", text: "Ops", stateBindingKey: "title" },
            { kind: "toggle", isOn: true, stateBindingKey: "enabled" },
          ],
        },
      ],
    });
  });

  test("parses ForEach collection element bindings for local state arrays", () => {
    const document = parseCustomSidebarSwift(
      `
      @State var tasks = [
        ["title": "Draft", "done": false, "points": 1],
        ["title": "Review", "done": true, "points": 2]
      ]

      VStack {
        ForEach($tasks) { $task in
          Text(task.title)
          TextField("Task", text: $task.title)
          Toggle("Done", isOn: $task.done)
          Stepper("Points", value: $task.points, in: 0...5)
        }
      }
      `,
      {
        sourceName: "bindings.swift",
        workspaceCount: 0,
        selectedTitle: "",
        selectedId: "",
        unreadTotal: 0,
        portTotal: 0,
      },
      [],
      {
        tasks: [
          { title: "Edited", done: false, points: 3 },
          { title: "Ship", done: true, points: 4 },
        ],
      },
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]).toEqual({ kind: "text", text: "Edited" });
    expect(root?.children[1]).toMatchObject({
      kind: "textField",
      text: "Edited",
      stateBindingKey: "tasks[0].title",
    });
    expect(root?.children[2]).toMatchObject({
      kind: "toggle",
      isOn: false,
      stateBindingKey: "tasks[0].done",
    });
    expect(root?.children[3]).toMatchObject({
      kind: "stepper",
      value: 3,
      stateBindingKey: "tasks[0].points",
    });
    expect(root?.children[4]).toEqual({ kind: "text", text: "Ship" });
    expect(root?.children[5]).toMatchObject({
      kind: "textField",
      text: "Ship",
      stateBindingKey: "tasks[1].title",
    });
  });

  test("parses local let bindings and user helper functions", () => {
    const document = parseCustomSidebarSwift(
      `
      let footer = selectedTitle

      func marker(_ workspace: WorkspacePreview) -> String {
        let symbol = workspace.selected ? "●" : "○"
        return symbol
      }

      func row(_ workspace: WorkspacePreview) -> some View {
        let state = workspace.selected ? "active" : "idle"
        HStack {
          Text(marker(workspace))
          Text(workspace.title)
          Text(state)
        }
      }

      VStack {
        ForEach(workspaces) { workspace in
          row(workspace)
        }
        Button("Report") {
          cmux("workspace.set_status", key: "marker", value: marker(workspaces[0]))
        }
        Text(footer)
      }
      `,
      {
        sourceName: "mine.swift",
        workspaceCount: 2,
        selectedTitle: "Ops",
        selectedId: "w1",
        unreadTotal: 0,
        portTotal: 0,
      },
      [
        workspacePreview({ id: "w1", index: 0, title: "Ops", selected: true }),
        workspacePreview({ id: "w2", index: 1, title: "API" }),
      ],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]).toEqual({
      kind: "hstack",
      children: [
        { kind: "text", text: "●" },
        { kind: "text", text: "Ops" },
        { kind: "text", text: "active" },
      ],
    });
    expect(root?.children[1]).toEqual({
      kind: "hstack",
      children: [
        { kind: "text", text: "○" },
        { kind: "text", text: "API" },
        { kind: "text", text: "idle" },
      ],
    });
    expect(root?.children[2]?.kind).toBe("button");
    if (root?.children[2]?.kind === "button") {
      expect(root.children[2].action).toEqual({
        method: "workspace.set_status",
        params: { key: "marker", value: "●" },
      });
    }
    expect(root?.children[3]).toEqual({ kind: "text", text: "Ops" });
  });

  test("parses constrained Swift environment reads", () => {
    const document = parseCustomSidebarSwift(
      `
      @Environment(\\.colorScheme) var colorScheme
      @Environment(\\.layoutDirection) private var layoutDirection
      @Environment(\\.locale) var locale

      VStack {
        Text("Scheme \\(colorScheme)")
        if colorScheme == .light {
          Text("Light branch")
        }
        if layoutDirection == .leftToRight {
          Text("LTR branch")
        }
        Text("Locale \\(locale.identifier)")
        Button("Report environment") {
          cmux(
            "workspace.set_metadata",
            scheme: colorScheme,
            rtl: layoutDirection == .rightToLeft,
            language: locale.languageCode
          )
        }
      }
      `,
      {
        sourceName: "environment.swift",
        workspaceCount: 0,
        selectedTitle: "",
        selectedId: "",
        unreadTotal: 0,
        portTotal: 0,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]).toEqual({ kind: "text", text: "Scheme light" });
    expect(root?.children[1]).toEqual({ kind: "text", text: "Light branch" });
    expect(root?.children[2]).toEqual({ kind: "text", text: "LTR branch" });
    expect(root?.children[3]).toEqual({ kind: "text", text: "Locale en-US" });
    expect(root?.children[4]?.kind).toBe("button");
    if (root?.children[4]?.kind === "button") {
      expect(root.children[4].action).toEqual({
        method: "workspace.set_metadata",
        params: {
          scheme: "light",
          rtl: false,
          language: "en",
        },
      });
    }
  });

  test("parses Swift string helpers and numeric builtins", () => {
    const document = parseCustomSidebarSwift(
      `
      VStack {
        if selectedTitle.hasPrefix("O") {
          Text(selectedTitle.uppercased())
        }
        ForEach(selectedTitle.split(separator: "p")) { part in
          Text(part.lowercased())
        }
        Text(String(min(workspaceCount, 1)))
        Text("Geometry \\(CGSize(width: 10, height: 20).width)x\\(CGRect(x: 1, y: 2, width: 30, height: 40).height)")
        Button("Helpers") {
          cmux(
            "workspace.set_metadata",
            title: selectedTitle.uppercased(),
            contains: selectedTitle.contains("p"),
            suffix: selectedTitle.hasSuffix("s"),
            count: selectedTitle.count,
            pieces: selectedTitle.split(separator: "p"),
            capped: min(workspaceCount, 1),
            floor: Int(2.8),
            ratio: Double("2.5"),
            magnitude: abs(-3)
          )
        }
      }
      `,
      {
        sourceName: "mine.swift",
        workspaceCount: 2,
        selectedTitle: "Ops",
        selectedId: "w1",
        unreadTotal: 0,
        portTotal: 0,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]).toEqual({ kind: "text", text: "OPS" });
    expect(root?.children[1]).toEqual({ kind: "text", text: "o" });
    expect(root?.children[2]).toEqual({ kind: "text", text: "s" });
    expect(root?.children[3]).toEqual({ kind: "text", text: "1" });
    expect(root?.children[4]).toEqual({ kind: "text", text: "Geometry 10x40" });
    expect(root?.children[5]?.kind).toBe("button");
    if (root?.children[5]?.kind === "button") {
      expect(root.children[5].action).toEqual({
        method: "workspace.set_metadata",
        params: {
          title: "OPS",
          contains: true,
          suffix: true,
          count: 3,
          pieces: ["O", "s"],
          capped: 1,
          floor: 2,
          ratio: 2.5,
          magnitude: 3,
        },
      });
    }
  });

  test("parses Swift Text markdown and verbatim literals", () => {
    const document = parseCustomSidebarSwift(
      `
      VStack {
        Text("**Ops** uses \`cmux\` and [docs](https://example.com)")
        Text(verbatim: "**literal**")
        Text("**\\(selectedTitle)**")
        Text("Open ") + Text(selectedTitle)
      }
      `,
      {
        sourceName: "mine.swift",
        workspaceCount: 1,
        selectedTitle: "Ops",
        selectedId: "w1",
        unreadTotal: 0,
        portTotal: 0,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]).toEqual({
      kind: "text",
      text: "**Ops** uses `cmux` and [docs](https://example.com)",
      markdownRuns: [
        { kind: "strong", text: "Ops" },
        { kind: "text", text: " uses " },
        { kind: "code", text: "cmux" },
        { kind: "text", text: " and " },
        { kind: "link", text: "docs", href: "https://example.com" },
      ],
    });
    expect(root?.children[1]).toEqual({ kind: "text", text: "**literal**" });
    expect(root?.children[2]).toEqual({ kind: "text", text: "**Ops**" });
    expect(root?.children[3]).toEqual({ kind: "text", text: "Open Ops" });
  });

  test("parses Swift Link views", () => {
    const document = parseCustomSidebarSwift(
      `
      VStack {
        Link("Docs", destination: URL(string: "https://example.com/docs")!)
        Link(destination: URL(string: "https://example.com/pr")) {
          Label("Pull request", systemImage: "arrow.up.right")
        }
        Link("Unsafe", destination: URL(string: "file:///tmp/secret")!)
      }
      `,
      {
        sourceName: "links.swift",
        workspaceCount: 1,
        selectedTitle: "Ops",
        selectedId: "w1",
        unreadTotal: 0,
        portTotal: 0,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]).toEqual({
      kind: "externalLink",
      title: "Docs",
      href: "https://example.com/docs",
      labelChildren: [],
    });
    expect(root?.children[1]).toEqual({
      kind: "externalLink",
      href: "https://example.com/pr",
      labelChildren: [{ kind: "label", text: "Pull request", systemImage: "arrow.up.right" }],
    });
    expect(root?.children[2]).toEqual({
      kind: "externalLink",
      title: "Unsafe",
      labelChildren: [],
    });
  });

  test("parses Swift ContentUnavailableView empty states", () => {
    const document = parseCustomSidebarSwift(
      `
      VStack {
        ContentUnavailableView(
          "No Workspaces",
          systemImage: "tray",
          description: Text("Create a workspace to begin")
        )
        ContentUnavailableView {
          Label("No Results", systemImage: "magnifyingglass")
        } description: {
          Text("Try another filter")
        } actions: {
          Button("Reload") { cmux("sidebar.reload", name: sourceName) }
        }
      }
      `,
      {
        sourceName: "empty.swift",
        workspaceCount: 0,
        selectedTitle: "",
        selectedId: "",
        unreadTotal: 0,
        portTotal: 0,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]).toEqual({
      kind: "contentUnavailable",
      title: "No Workspaces",
      systemImage: "tray",
      labelChildren: [],
      descriptionChildren: [{ kind: "text", text: "Create a workspace to begin" }],
      actionsChildren: [],
    });
    expect(root?.children[1]).toMatchObject({
      kind: "contentUnavailable",
      labelChildren: [{ kind: "label", text: "No Results", systemImage: "magnifyingglass" }],
      descriptionChildren: [{ kind: "text", text: "Try another filter" }],
      actionsChildren: [
        {
          kind: "button",
          role: undefined,
          text: "Reload",
          children: [],
          action: {
            method: "sidebar.reload",
            params: { name: "empty.swift" },
          },
        },
      ],
    });
  });

  test("parses Swift event bridge context", () => {
    const events = customSidebarEventsContextFromSnapshot({
      latest: {
        category: "workspace",
        name: "workspace.selected",
        seq: 12,
      },
      latest_seq: 12,
      name_counts: {
        "workspace.selected": 3,
      },
      recent: [
        { category: "session", name: "session.changed", seq: 11 },
        { category: "workspace", name: "workspace.selected", seq: 12 },
      ],
    });
    const document = parseCustomSidebarSwift(
      `
      VStack {
        Text(events.latest.name)
        Text("\\(events.recent.count)")
        Text("\\(events.name_counts["workspace.selected"])")
      }
      `,
      {
        sourceName: "mine.swift",
        workspaceCount: 1,
        selectedTitle: "Ops",
        selectedId: "w1",
        unreadTotal: 0,
        portTotal: 0,
        events,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]).toEqual({
      kind: "text",
      text: "workspace.selected",
    });
    expect(root?.children[1]).toEqual({ kind: "text", text: "2" });
    expect(root?.children[2]).toEqual({ kind: "text", text: "3" });
  });

  test("parses Swift onEvent handlers for local state and safe actions", () => {
    const events = customSidebarEventsContextFromSnapshot({
      latest: {
        seq: 8,
        name: "workspace.selected",
        category: "workspace",
        payload: { workspace_id: "w2" },
      },
      recent: [
        { seq: 8, name: "workspace.selected", category: "workspace" },
      ],
      latest_seq: 8,
      oldest_seq: 8,
      next_seq: 9,
      retained_count: 1,
      boot_id: "boot-1",
    });
    const document = parseCustomSidebarSwift(
      `
        @State var lastEvent = "none"
        @State var latestCategory = "none"
        VStack {
          Text(lastEvent)
            .onEvent("workspace.selected") {
              lastEvent = events.latest.name
              cmux("workspace.set_status", key: "event", value: events.latest.name)
            }
          Text(latestCategory)
            .onEvent(category: "workspace") {
              latestCategory = events.latest.category
            }
        }
      `,
      {
        sourceName: "events.swift",
        workspaceCount: 0,
        selectedTitle: "",
        selectedId: "",
        unreadTotal: 0,
        portTotal: 0,
        events,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.eventHandlers).toEqual([
      {
        eventName: "workspace.selected",
        assignments: [{ key: "lastEvent", value: "workspace.selected" }],
        action: {
          method: "workspace.set_status",
          params: { key: "event", value: "workspace.selected" },
        },
      },
      {
        eventCategory: "workspace",
        assignments: [{ key: "latestCategory", value: "workspace" }],
      },
    ]);
    expect(
      customSidebarSwiftEventHandlerMatches(document.eventHandlers[0]!, {
        name: "workspace.selected",
        category: "workspace",
      }),
    ).toBe(true);
    expect(
      customSidebarSwiftEventHandlerMatches(document.eventHandlers[0]!, {
        name: "workspace.renamed",
        category: "workspace",
      }),
    ).toBe(false);
    expect(
      customSidebarSwiftEventHandlerMatches(document.eventHandlers[1]!, {
        name: "workspace.renamed",
        category: "workspace",
      }),
    ).toBe(true);
  });

  test("parses Swift onSubmit and onChange local handlers", () => {
    const document = parseCustomSidebarSwift(
      `
        @State var draft = "Draft"
        @State var status = "idle"
        @State var rowFocused = false
        VStack {
          TextField("Draft", text: $draft)
            .onSubmit {
              status = "submitted"
              cmux("sidebar.reload", name: sourceName)
            }
            .onChange(of: draft) {
              status = "changed"
            }
          Text("Lifecycle")
            .onAppear {
              status = "appeared"
            }
            .onDisappear {
              status = "gone"
            }
        Text("Task")
          .task(id: selectedId) {
            status = "tasked"
            cmux("sidebar.reload", name: sourceName)
          }
        Text("Hover")
          .onHover { isHovering in
            status = isHovering ? "hovering" : "idle"
          }
        Text("Focus")
          .focusable()
          .focused($rowFocused)
      }
      `,
      {
        sourceName: "submit.swift",
        workspaceCount: 0,
        selectedTitle: "",
        selectedId: "workspace-a",
        unreadTotal: 0,
        portTotal: 0,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]).toMatchObject({
      kind: "textField",
      text: "Draft",
      stateBindingKey: "draft",
      modifiers: [
        {
          name: "onSubmit",
          localHandler: {
            assignments: [{ key: "status", value: "submitted" }],
            action: {
              method: "sidebar.reload",
              params: { name: "submit.swift" },
            },
          },
        },
        {
          name: "onChange",
          value: "Draft",
          secondaryValue: "draft",
          localHandler: {
            assignments: [{ key: "status", value: "changed" }],
          },
        },
      ],
    });
    expect(root?.children[1]).toMatchObject({
      kind: "text",
      text: "Lifecycle",
      modifiers: [
        {
          name: "onAppear",
          localHandler: {
            assignments: [{ key: "status", value: "appeared" }],
          },
        },
        {
          name: "onDisappear",
          localHandler: {
            assignments: [{ key: "status", value: "gone" }],
          },
        },
      ],
    });
    expect(root?.children[2]).toMatchObject({
      kind: "text",
      text: "Task",
      modifiers: [
        {
          name: "task",
          value: "workspace-a",
          secondaryValue: "selectedId",
          localHandler: {
            assignments: [{ key: "status", value: "tasked" }],
            action: {
              method: "sidebar.reload",
              params: { name: "submit.swift" },
            },
          },
        },
      ],
    });
    expect(root?.children[3]).toMatchObject({
      kind: "text",
      text: "Hover",
      modifiers: [
        {
          name: "onHover",
          value: "isHovering",
          localHandler: {
            assignments: [{ key: "status", value: "hovering" }],
          },
          falseLocalHandler: {
            assignments: [{ key: "status", value: "idle" }],
          },
        },
      ],
    });
    expect(root?.children[4]).toMatchObject({
      kind: "text",
      text: "Focus",
      modifiers: [
        {
          name: "focusable",
          boolValue: true,
        },
        {
          name: "focused",
          value: "$rowFocused",
          stateBindingKey: "rowFocused",
          boolValue: false,
        },
      ],
    });
    expect(document.eventHandlers).toEqual([]);
  });

  test("parses Swift arithmetic, comparisons, and logical expressions", () => {
    const document = parseCustomSidebarSwift(
      `
      VStack {
        if workspaceCount + 1 >= 3 && selectedTitle.hasPrefix("O") {
          Text(String((workspaceCount + 2) * 3))
        }
        if !workspaces[1].selected || selectedTitle == "Ops" {
          Text("logic")
        }
        Text("Lane " + selectedTitle)
        Button("Math") {
          cmux(
            "workspace.set_metadata",
            sum: workspaceCount + 2,
            product: (workspaceCount + 1) * 4,
            remainder: 7 % 3,
            safe: 4 / 0,
            greater: workspaceCount > 1,
            different: selectedTitle != "API",
            combined: selectedTitle.hasSuffix("s") && workspaceCount <= 2
          )
        }
      }
      `,
      {
        sourceName: "mine.swift",
        workspaceCount: 2,
        selectedTitle: "Ops",
        selectedId: "w1",
        unreadTotal: 0,
        portTotal: 0,
      },
      [
        workspacePreview({ id: "w1", index: 0, title: "Ops", selected: true }),
        workspacePreview({ id: "w2", index: 1, title: "API" }),
      ],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]).toEqual({ kind: "text", text: "12" });
    expect(root?.children[1]).toEqual({ kind: "text", text: "logic" });
    expect(root?.children[2]).toEqual({ kind: "text", text: "Lane Ops" });
    expect(root?.children[3]?.kind).toBe("button");
    if (root?.children[3]?.kind === "button") {
      expect(root.children[3].action).toEqual({
        method: "workspace.set_metadata",
        params: {
          sum: 4,
          product: 12,
          remainder: 1,
          safe: 0,
          greater: true,
          different: true,
          combined: true,
        },
      });
    }
  });

  test("parses Swift dictionary literals and keyed subscripts", () => {
    const document = parseCustomSidebarSwift(
      `
      let labels = [selectedId: selectedTitle, "fallback": "Idle"]
      let counts = ["workspaces": workspaceCount, "ports": portTotal + 1]
      VStack {
        Text(labels[workspaces[0].id])
        Text(String(counts["ports"]))
        Button("Dictionary") {
          cmux(
            "workspace.set_metadata",
            payload: ["label": labels[selectedId], "count": counts["workspaces"]],
            missing: labels["missing"] == nil ? "none" : "present"
          )
        }
      }
      `,
      {
        sourceName: "mine.swift",
        workspaceCount: 2,
        selectedTitle: "Ops",
        selectedId: "w1",
        unreadTotal: 0,
        portTotal: 4,
      },
      [
        workspacePreview({ id: "w1", index: 0, title: "Ops", selected: true }),
      ],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]).toEqual({ kind: "text", text: "Ops" });
    expect(root?.children[1]).toEqual({ kind: "text", text: "5" });
    expect(root?.children[2]?.kind).toBe("button");
    if (root?.children[2]?.kind === "button") {
      expect(root.children[2].action).toEqual({
        method: "workspace.set_metadata",
        params: {
          payload: { label: "Ops", count: 2 },
          missing: "none",
        },
      });
    }
  });

  test("parses Swift collection transform closures", () => {
    const document = parseCustomSidebarSwift(
      `
      let grouped = Dictionary(grouping: workspaces, by: \\.selected)
      let groupedByClosure = Dictionary(grouping: workspaces, by: { $0.title.hasPrefix("O") })
      VStack {
        ForEach(workspaces.filter { $0.selected }) { workspace in
          Text("Selected \\(workspace.title)")
        }
        ForEach(workspaces.sorted { $0.title < $1.title }) { workspace in
          Text("Sorted \\(workspace.title)")
        }
        ForEach(workspaces.map(\\.title), id: \\.self) { title in
          Text("Mapped \\(title)")
        }
        ForEach(workspaces.sorted(by: \\.title)) { workspace in
          Text("KeySorted \\(workspace.title)")
        }
        Text(String(workspaces.map { $0.title }.count))
        Text(String(workspaces.compactMap(\\.progress).count))
        Text(String(workspaces.flatMap(\\.ports).count))
        Text("Least unread \\(workspaces.min(by: \\.unreadCount).title)")
        Text("Most unread \\(workspaces.max(by: \\.unreadCount).title)")
        Text("All selected \\(String(workspaces.allSatisfy(\\.selected)))")
        Text("Selected group \\(grouped[true].count)")
        Text("Closure group \\(groupedByClosure[true].count)")
        Button("Transforms") {
          cmux(
            "workspace.set_metadata",
            titles: workspaces.sorted { $0.title < $1.title }.map(\\.title),
            selectedIds: workspaces.filter { workspace in workspace.selected }.map(\\.id),
            progresses: workspaces.compactMap(\\.progress),
            allPorts: workspaces.flatMap(\\.ports),
            leastUnreadTitle: workspaces.min(by: \\.unreadCount).title,
            mostUnreadTitle: workspaces.max(by: \\.unreadCount).title,
            allSelected: workspaces.allSatisfy(\\.selected),
            selectedGroupCount: grouped[true].count
          )
        }
      }
      `,
      {
        sourceName: "mine.swift",
        workspaceCount: 2,
        selectedTitle: "Ops",
        selectedId: "w1",
        unreadTotal: 0,
        portTotal: 0,
      },
      [
        workspacePreview({
          id: "w1",
          index: 0,
          title: "Ops",
          selected: true,
          unreadCount: 2,
          ports: [3000],
          progress: 0.75,
        }),
        workspacePreview({
          id: "w2",
          index: 1,
          title: "API",
          unreadCount: 5,
          ports: [4000, 5000],
        }),
      ],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]).toEqual({ kind: "text", text: "Selected Ops" });
    expect(root?.children[1]).toEqual({ kind: "text", text: "Sorted API" });
    expect(root?.children[2]).toEqual({ kind: "text", text: "Sorted Ops" });
    expect(root?.children[3]).toEqual({
      kind: "text",
      text: "Mapped Ops",
      modifiers: [{ name: "id", value: "Ops" }],
    });
    expect(root?.children[4]).toEqual({
      kind: "text",
      text: "Mapped API",
      modifiers: [{ name: "id", value: "API" }],
    });
    expect(root?.children[5]).toEqual({ kind: "text", text: "KeySorted API" });
    expect(root?.children[6]).toEqual({ kind: "text", text: "KeySorted Ops" });
    expect(root?.children[7]).toEqual({ kind: "text", text: "2" });
    expect(root?.children[8]).toEqual({ kind: "text", text: "1" });
    expect(root?.children[9]).toEqual({ kind: "text", text: "3" });
    expect(root?.children[10]).toEqual({ kind: "text", text: "Least unread Ops" });
    expect(root?.children[11]).toEqual({ kind: "text", text: "Most unread API" });
    expect(root?.children[12]).toEqual({ kind: "text", text: "All selected false" });
    expect(root?.children[13]).toEqual({ kind: "text", text: "Selected group 1" });
    expect(root?.children[14]).toEqual({ kind: "text", text: "Closure group 1" });
    expect(root?.children[15]?.kind).toBe("button");
    if (root?.children[15]?.kind === "button") {
      expect(root.children[15].action).toEqual({
        method: "workspace.set_metadata",
        params: {
          titles: ["API", "Ops"],
          selectedIds: ["w1"],
          progresses: [0.75],
          allPorts: [3000, 4000, 5000],
          leastUnreadTitle: "Ops",
          mostUnreadTitle: "API",
          allSelected: false,
          selectedGroupCount: 1,
        },
      });
    }
  });

  test("parses Swift reduce and numeric formatting helpers", () => {
    const document = parseCustomSidebarSwift(
      `
      let totalUnread = workspaces.reduce(0) { total, workspace in
        total + workspace.unreadCount
      }
      let ratio = 0.25
      let monthly = 12
      let bytes = 1536
      let distance = Measurement(value: 12.5, unit: UnitLength.kilometers)
      let duration = Measurement(value: 3, unit: UnitDuration.hours)
      let launched = Date(timeIntervalSince1970: 1704067200)
      let intervalStart = Date(timeIntervalSince1970: 1704067200)
      let intervalEnd = Date(timeIntervalSince1970: 1704070800)
      let names = ["Ops", "API", "Docs"]
      VStack {
        Text(String(totalUnread))
        Text(ratio.formatted(.percent))
        Text(monthly.formatted(.currency(code: "USD")))
        Text(portTotal, format: .notation(.compactName))
        Text(String(format: "%.1f%%", ratio * 100))
        Text(String(format: "%03d %@", arguments: [workspaceCount, selectedTitle]))
        Text(bytes.formatted(.byteCount(style: .file)))
        Text(portTotal, format: .byteCount(style: .file))
        Text(distance.formatted(.measurement(width: .abbreviated)))
        Text(duration, format: .measurement(width: .wide))
        Text(launched.formatted(.dateTime.year().month().day()))
        Text(launched, format: .dateTime.hour().minute())
        Text(launched, style: .date)
        Text(launched, style: .time)
        Text(timerInterval: intervalStart...intervalEnd, countsDown: true)
        Text(names.formatted(.list(type: .and)))
        Text(names, format: .list(type: .or))
        Button("Summary") {
          cmux(
            "workspace.set_metadata",
            unread: workspaces.reduce(0) { $0 + $1.unreadCount },
            compactPorts: portTotal.formatted(.notation(.compactName)),
            formattedRatio: String(format: "%.2f", ratio),
            formattedBytes: bytes.formatted(.byteCount(style: .file)),
            formattedDistance: distance.formatted(.measurement(width: .abbreviated)),
            formattedDate: launched.formatted(.dateTime.year().month().day()),
            formattedNames: names.formatted(.list(type: .and))
          )
        }
      }
      `,
      {
        sourceName: "mine.swift",
        workspaceCount: 2,
        selectedTitle: "Ops",
        selectedId: "w1",
        unreadTotal: 7,
        portTotal: 1200,
      },
      [
        workspacePreview({ id: "w1", index: 0, title: "Ops", unreadCount: 2 }),
        workspacePreview({ id: "w2", index: 1, title: "API", unreadCount: 5 }),
      ],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]).toEqual({ kind: "text", text: "7" });
    expect(root?.children[1]).toEqual({ kind: "text", text: "25%" });
    expect(root?.children[2]).toEqual({ kind: "text", text: "$12.00" });
    expect(root?.children[3]).toEqual({ kind: "text", text: "1.2K" });
    expect(root?.children[4]).toEqual({ kind: "text", text: "25.0%" });
    expect(root?.children[5]).toEqual({ kind: "text", text: "002 Ops" });
    expect(root?.children[6]).toEqual({ kind: "text", text: "1.5 KB" });
    expect(root?.children[7]).toEqual({ kind: "text", text: "1.2 KB" });
    expect(root?.children[8]).toEqual({ kind: "text", text: "12.5 km" });
    expect(root?.children[9]).toEqual({ kind: "text", text: "3 hours" });
    expect(root?.children[10]).toEqual({ kind: "text", text: "Jan 1, 2024" });
    expect(root?.children[11]).toEqual({ kind: "text", text: "12:00 AM" });
    expect(root?.children[12]).toEqual({
      kind: "text",
      text: "Jan 1, 2024",
      textStyle: "date",
    });
    expect(root?.children[13]).toEqual({
      kind: "text",
      text: "12:00 AM",
      textStyle: "time",
    });
    expect(root?.children[14]).toEqual({
      kind: "text",
      text: "1:00:00",
      textStyle: "timer",
      timerIntervalStartMs: 1704067200000,
      timerIntervalEndMs: 1704070800000,
      timerCountsDown: true,
    });
    expect(root?.children[15]).toEqual({ kind: "text", text: "Ops, API, and Docs" });
    expect(root?.children[16]).toEqual({ kind: "text", text: "Ops, API, or Docs" });
    expect(root?.children[17]?.kind).toBe("button");
    if (root?.children[17]?.kind === "button") {
      expect(root.children[17].action).toEqual({
        method: "workspace.set_metadata",
        params: {
          unread: 7,
          compactPorts: "1.2K",
          formattedRatio: "0.25",
          formattedBytes: "1.5 KB",
          formattedDistance: "12.5 km",
          formattedDate: "Jan 1, 2024",
          formattedNames: "Ops, API, and Docs",
        },
      });
    }
  });
});

describe("CustomSidebarSurface", () => {
  test("renders the selected workspace and live session facts", () => {
    currentSelectedWorkspaceIndex = 1;
    currentWorkspaces = [
      workspace({
        workspace_id: "one",
        process_title: "API",
        listening_ports: [3000],
      }),
      workspace({
        workspace_id: "two",
        custom_title: "Review lane",
        current_directory: "C:\\repo\\feature",
        layout: {
          type: "split",
          split: {
            orientation: "horizontal",
            divider_position: 0.5,
            first: { type: "pane", pane: { panel_ids: ["left"] } },
            second: { type: "pane", pane: { panel_ids: ["right"] } },
          },
        },
        panel_unreads: [{ panel_id: "right", is_unread: true }],
        panel_git_branches: [
          { panel_id: "left", branch: "feature/custom-sidebar", is_dirty: true },
        ],
        panel_listening_ports: [{ panel_id: "right", ports: [5173, 5173] }],
        sidebar_progress: { value: 0.42, label: "Rendering" },
        sidebar_status_entries: [{ key: "ci", value: "running", updated_at: 1 }],
        sidebar_metadata_blocks: [{ key: "notes", markdown: "Hello", updated_at: 1 }],
        sidebar_log_entries: [{ message: "Started", level: "info", created_at: 1 }],
        remote: {
          enabled: true,
          state: "connected",
          connected: true,
          has_ssh_options: false,
          detected_ports: [],
          forwarded_ports: [],
          conflicted_ports: [],
        },
      }),
    ];

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\review.swift" />,
    );

    expect(markup).toContain("cmux-custom-sidebar-surface");
    expect(markup).toContain("review.swift");
    expect(markup).toContain("Swift");
    expect(markup).toContain("2 workspaces");
    expect(markup).toContain("1 unread");
    expect(markup).toContain("Review lane");
    expect(markup).toContain("C:\\repo\\feature");
    expect(markup).toContain("feature/custom-sidebar (dirty)");
    expect(markup).toContain("5173");
    expect(markup).toContain("42%");
    expect(markup).toContain("connected");
    expect(markup).toContain("Windows/Tauri Swift subset renderer");
  });

  test("renders an empty-state preview without workspaces", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [];

    const markup = renderToStaticMarkup(<CustomSidebarSurface />);

    expect(markup).toContain("Unsaved custom sidebar");
    expect(markup).toContain("No workspaces in this session yet.");
  });

  test("renders authored JSON blocks against live session data", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [
      workspace({
        workspace_id: "workspace-a",
        custom_title: "Ops lane",
        panel_titles: [{ panel_id: "surface-1", custom_title: "Server log" }],
        panel_listening_ports: [{ panel_id: "surface-1", ports: [3000] }],
        panel_unreads: [{ panel_id: "surface-1", is_unread: true }],
      }),
    ];
    const source = JSON.stringify({
      title: "Board for {selectedTitle}",
      subtitle: "{workspaceCount} workspace, {unreadTotal} unread",
      blocks: [
        { type: "heading", text: "Live board" },
        { type: "stat", label: "Ports", value: "{portTotal}" },
        {
          type: "button",
          label: "Mark {selectedTitle}",
          detail: "Runs through the shared dispatcher",
          action: {
            method: "workspace.set_status",
            params: { key: "json", value: "from {sourceName}" },
          },
        },
        { type: "workspaceList", title: "Unread", filter: "unread" },
        { type: "selectedTabs", title: "Tabs" },
      ],
      footer: "Rendered from {sourceName}",
    });

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\ops.json"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("JSON custom sidebar");
    expect(markup).toContain("Board for Ops lane");
    expect(markup).toContain("1 workspace, 1 unread");
    expect(markup).toContain("Live board");
    expect(markup).toContain("Ports");
    expect(markup).toContain("<strong>1</strong>");
    expect(markup).toContain("Mark Ops lane");
    expect(markup).toContain("Runs through the shared dispatcher");
    expect(markup).toContain("Unread");
    expect(markup).toContain("Ops lane");
    expect(markup).toContain("Tabs");
    expect(markup).toContain("Server log");
    expect(markup).toContain("Rendered from ops.json");
  });

  test("renders authored Swift subset against live session data", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [
      workspace({
        workspace_id: "workspace-a",
        custom_title: "Ops lane",
      }),
      workspace({
        workspace_id: "workspace-b",
        custom_title: "API lane",
      }),
    ];
    const source = `
      VStack(spacing: 8) {
        Text("Board for \\(selectedTitle)")
        Divider()
        ForEach(workspaces) { w in
          Button(action: { cmux("workspace.select", workspace_id: w.id) }) {
            HStack { Text(w.selected ? "●" : "○"); Text(w.title); Spacer(minLength: 24) }
          }
        }
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\mine.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("Swift custom sidebar");
    expect(markup).toContain("Board for Ops lane");
    expect(markup).toContain("Ops lane");
    expect(markup).toContain("API lane");
    expect(markup).toContain("cmux-custom-sidebar-swift-button");
    expect(markup).toContain("cmux-custom-sidebar-swift-spacer");
    expect(markup).toContain('data-swift-spacer-min-length="24"');
    expect(markup).toContain("min-width:24px");
    expect(markup).toContain("min-height:24px");
    expect(markup).toContain("Windows/Tauri Swift subset renderer");
  });

  test("parses Swift list, section, labels, progress, indices, and ranges", () => {
    const document = parseCustomSidebarSwift(
      `
        List {
          Section("Repos") {
            Label(workspaces[0].title, systemImage: "folder")
            Label(title: { Text("Route") }, icon: { Image(systemName: "arrow.right") })
            ProgressView(value: workspaces[0].progress, total: 1)
            ForEach(workspaces.indices) { i in
              Text("\\(i): \\(workspaces[i].title)")
            }
            for n in 0..<2 {
              Image(systemName: n == 0 ? "checkmark.circle" : "clock")
            }
          }
          Section(header: Text("Summary"), footer: Text("Updated live")) {
            Text(selectedTitle)
          }
        }
      `,
      {
        sourceName: "ops.swift",
        workspaceCount: 2,
        selectedTitle: "Ops lane",
        selectedId: "workspace-a",
        unreadTotal: 0,
        portTotal: 0,
      },
      [
        {
          id: "workspace-a",
          index: 0,
          title: "Ops lane",
          tabs: [],
          tabCount: 0,
          unreadCount: 0,
          ports: [],
          dirty: false,
          progress: 0.5,
          statusCount: 0,
          metadataCount: 0,
          logCount: 0,
          selected: true,
        },
        {
          id: "workspace-b",
          index: 1,
          title: "API lane",
          tabs: [],
          tabCount: 0,
          unreadCount: 0,
          ports: [],
          dirty: false,
          statusCount: 0,
          metadataCount: 0,
          logCount: 0,
          selected: false,
        },
      ],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("list");
    const list = document.root.kind === "list" ? document.root : undefined;
    expect(list?.children[0]?.kind).toBe("section");
    const section = list?.children[0]?.kind === "section" ? list.children[0] : undefined;
    expect(section?.title).toBe("Repos");
    expect(section?.children).toEqual([
      { kind: "label", text: "Ops lane", systemImage: "folder" },
      { kind: "label", text: "Route", systemImage: "arrow.right" },
      { kind: "progress", value: 0.5, total: 1 },
      { kind: "text", text: "0: Ops lane" },
      { kind: "text", text: "1: API lane" },
      { kind: "image", systemName: "checkmark.circle" },
      { kind: "image", systemName: "clock" },
    ]);
    expect(list?.children[1]).toMatchObject({
      kind: "section",
      header: [{ kind: "text", text: "Summary" }],
      footer: [{ kind: "text", text: "Updated live" }],
      children: [{ kind: "text", text: "Ops lane" }],
    });

    const dataListDocument = parseCustomSidebarSwift(
      `
        List(workspaces, id: \\.id) { workspace in
          Text(workspace.title)
        }
      `,
      {
        sourceName: "ops.swift",
        workspaceCount: 2,
        selectedTitle: "Ops lane",
        selectedId: "workspace-a",
        unreadTotal: 0,
        portTotal: 0,
      },
      [
        {
          id: "workspace-a",
          index: 0,
          title: "Ops lane",
          tabs: [],
          tabCount: 0,
          unreadCount: 0,
          ports: [],
          dirty: false,
          statusCount: 0,
          metadataCount: 0,
          logCount: 0,
          selected: true,
        },
        {
          id: "workspace-b",
          index: 1,
          title: "API lane",
          tabs: [],
          tabCount: 0,
          unreadCount: 0,
          ports: [],
          dirty: false,
          statusCount: 0,
          metadataCount: 0,
          logCount: 0,
          selected: false,
        },
      ],
    );

    expect(dataListDocument.warnings).toEqual([]);
    expect(dataListDocument.root).toMatchObject({
      kind: "list",
      dataId: "id",
      children: [
        { kind: "text", text: "Ops lane" },
        { kind: "text", text: "API lane" },
      ],
    });
  });

  test("parses Swift Gauge, AnyView, and ViewThatFits wrappers", () => {
    const document = parseCustomSidebarSwift(
      `
        VStack {
          Gauge(value: workspaces[0].progress, in: 0...1)
          AnyView(Text(selectedTitle))
          ViewThatFits(in: .horizontal) {
            Text("Compact")
            Text("Wide")
          }
          TabView {
            Text("First page")
              .tabItem { Text("Overview") }
            Text("Second page")
              .tabItem { Label("Details", systemImage: "list.bullet") }
          }
            .tabViewStyle(.page)
          Group {
            Text("Grouped")
          }
          EmptyView()
        }
      `,
      {
        sourceName: "ops.swift",
        workspaceCount: 1,
        selectedTitle: "Ops lane",
        selectedId: "workspace-a",
        unreadTotal: 0,
        portTotal: 0,
      },
      [workspacePreview({ id: "workspace-a", title: "Ops lane", progress: 0.42 })],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]).toEqual({ kind: "progress", value: 0.42, total: 1 });
    expect(root?.children[1]).toEqual({ kind: "text", text: "Ops lane" });
    expect(root?.children[2]).toEqual({
      kind: "group",
      groupRole: "viewThatFits",
      fitAxis: "horizontal",
      children: [
        { kind: "text", text: "Compact" },
        { kind: "text", text: "Wide" },
      ],
    });
    expect(root?.children[3]).toEqual({
      kind: "tabView",
      children: [
        {
          kind: "modified",
          base: { kind: "text", text: "First page" },
          childModifiers: [
            { name: "tabItem", children: [{ kind: "text", text: "Overview" }] },
          ],
        },
        {
          kind: "modified",
          base: { kind: "text", text: "Second page" },
          childModifiers: [
            {
              name: "tabItem",
              children: [{ kind: "label", text: "Details", systemImage: "list.bullet" }],
            },
          ],
        },
      ],
      modifiers: [{ name: "tabViewStyle", value: "page" }],
    });
    expect(root?.children[4]).toEqual({
      kind: "group",
      groupRole: "group",
      children: [{ kind: "text", text: "Grouped" }],
    });
    expect(root?.children[5]).toEqual({ kind: "empty" });
  });

  test("parses Swift LabeledContent rows", () => {
    const document = parseCustomSidebarSwift(
      `
        VStack {
          LabeledContent("Workspace", value: selectedTitle)
          LabeledContent("Status") {
            Text("\\(workspaceCount) workspaces")
            Label("Healthy", systemImage: "checkmark.circle")
          }
          GroupBox("Summary") {
            Text(selectedTitle)
          }
            .groupBoxStyle(.card)
          GroupBox(label: {
            Label("Health", systemImage: "heart")
          }) {
            Text("Nominal")
          }
        }
      `,
      {
        sourceName: "ops.swift",
        workspaceCount: 2,
        selectedTitle: "Ops lane",
        selectedId: "workspace-a",
        unreadTotal: 0,
        portTotal: 0,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]).toEqual({
      kind: "labeledContent",
      title: "Workspace",
      value: "Ops lane",
      children: [],
    });
    expect(root?.children[1]).toEqual({
      kind: "labeledContent",
      title: "Status",
      children: [
        { kind: "text", text: "2 workspaces" },
        { kind: "label", text: "Healthy", systemImage: "checkmark.circle" },
      ],
    });
    expect(root?.children[2]).toEqual({
      kind: "groupBox",
      title: "Summary",
      labelChildren: [],
      children: [{ kind: "text", text: "Ops lane" }],
      modifiers: [{ name: "groupBoxStyle", value: "card" }],
    });
    expect(root?.children[3]).toEqual({
      kind: "groupBox",
      labelChildren: [{ kind: "label", text: "Health", systemImage: "heart" }],
      children: [{ kind: "text", text: "Nominal" }],
    });
  });

  test("parses Swift DisclosureGroup containers", () => {
    const document = parseCustomSidebarSwift(
      `
        @State var advancedOpen = false

        VStack {
          DisclosureGroup("Ports") {
            Text("\\(portTotal) ports")
          }
          DisclosureGroup(isExpanded: $advancedOpen) {
            Text("Advanced controls")
          } label: {
            Label("Advanced", systemImage: "gearshape")
          }
        }
      `,
      {
        sourceName: "disclosure.swift",
        workspaceCount: 1,
        selectedTitle: "Ops lane",
        selectedId: "workspace-a",
        unreadTotal: 0,
        portTotal: 3,
      },
      [],
      { advancedOpen: true },
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]).toEqual({
      kind: "disclosureGroup",
      title: "Ports",
      isExpanded: true,
      labelChildren: [],
      children: [{ kind: "text", text: "3 ports" }],
    });
    expect(root?.children[1]).toEqual({
      kind: "disclosureGroup",
      isExpanded: true,
      stateBindingKey: "advancedOpen",
      labelChildren: [{ kind: "label", text: "Advanced", systemImage: "gearshape" }],
      children: [{ kind: "text", text: "Advanced controls" }],
    });
  });

  test("parses common SwiftUI modifiers into render metadata", () => {
    const document = parseCustomSidebarSwift(
      `
        VStack(alignment: .leading, spacing: 8) {
          Text("Ops")
            .font(.title3)
            .bold()
            .foregroundColor(.secondary)
            .id("ops-heading")
          Text("Subtle")
            .foregroundStyle(.tertiary)
          Text("Accent")
            .tint(.accent)
          Text("Tracked")
            .tracking(1.5)
          Text("Kerned")
            .kerning(2)
          Text("Raised")
            .baselineOffset(3)
          Text("Sized")
            .frame(width: 120, height: 32, minWidth: 80, idealHeight: 28, maxHeight: 44, alignment: .trailing)
          HStack(alignment: .top) {
            Text("Fill")
          }
            .padding(12)
            .padding(.horizontal, 6)
            .padding([.top, .bottom], 4)
            .background(.blue)
            .cornerRadius(10)
            .frame(maxWidth: .infinity, alignment: .trailing)
          HStack {
            Text("Insets")
          }
            .padding(EdgeInsets(top: 1, leading: 2, bottom: 3, trailing: 4))
          HStack {
            Text("Safe")
          }
            .safeAreaPadding(.horizontal, 12)
          HStack {
            Text("Content")
          }
            .contentMargins(.bottom, 5, for: .scrollContent)
          HStack {
            Text("Safe Insets")
          }
            .safeAreaPadding(EdgeInsets(top: 2, leading: 3, bottom: 4, trailing: 5))
        }
      `,
      {
        sourceName: "ops.swift",
        workspaceCount: 0,
        selectedTitle: "Ops",
        selectedId: "",
        unreadTotal: 0,
        portTotal: 0,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.alignment).toBe("leading");
    expect(root?.spacing).toBe(8);
    expect(root?.children[0]).toEqual({
      kind: "text",
      text: "Ops",
      modifiers: [
        { name: "font", value: "title3" },
        { name: "bold" },
        { name: "foregroundColor", value: "secondary" },
        { name: "id", value: "ops-heading" },
      ],
    });
    expect(root?.children[1]).toEqual({
      kind: "text",
      text: "Subtle",
      modifiers: [{ name: "foregroundColor", value: "tertiary" }],
    });
    expect(root?.children[2]).toEqual({
      kind: "text",
      text: "Accent",
      modifiers: [{ name: "foregroundColor", value: "accent" }],
    });
    expect(root?.children[3]).toEqual({
      kind: "text",
      text: "Tracked",
      modifiers: [{ name: "tracking", value: "1.5" }],
    });
    expect(root?.children[4]).toEqual({
      kind: "text",
      text: "Kerned",
      modifiers: [{ name: "kerning", value: "2" }],
    });
    expect(root?.children[5]).toEqual({
      kind: "text",
      text: "Raised",
      modifiers: [{ name: "baselineOffset", value: "3" }],
    });
    expect(root?.children[6]).toEqual({
      kind: "text",
      text: "Sized",
      modifiers: [
        {
          name: "frame",
          frameWidth: 120,
          frameHeight: 32,
          frameMinWidth: 80,
          frameMaxHeight: 44,
          frameIdealHeight: 28,
          frameAlignment: "trailing",
          maxWidthInfinity: false,
        },
      ],
    });
    expect(root?.children[7]?.modifiers).toEqual([
      { name: "padding", value: "12" },
      { name: "padding", value: "6", edge: "leading,trailing" },
      { name: "padding", value: "4", edge: "top,bottom" },
      { name: "background", value: "blue" },
      { name: "cornerRadius", value: "10" },
      { name: "frame", frameAlignment: "trailing", maxWidthInfinity: true },
    ]);
    expect(root?.children[7]).toMatchObject({
      kind: "hstack",
      alignment: "top",
    });
    expect(root?.children[8]?.modifiers).toEqual([
      {
        name: "padding",
        paddingTop: 1,
        paddingLeading: 2,
        paddingBottom: 3,
        paddingTrailing: 4,
      },
    ]);
    expect(root?.children[9]?.modifiers).toEqual([
      { name: "safeAreaPadding", value: "12", edge: "leading,trailing" },
    ]);
    expect(root?.children[10]?.modifiers).toEqual([
      { name: "contentMargins", value: "5", edge: "bottom", placement: "scrollContent" },
    ]);
    expect(root?.children[11]?.modifiers).toEqual([
      {
        name: "safeAreaPadding",
        paddingTop: 2,
        paddingLeading: 3,
        paddingBottom: 4,
        paddingTrailing: 5,
      },
    ]);
  });

  test("parses SwiftUI child-bearing modifiers", () => {
    const document = parseCustomSidebarSwift(
      `
        @State var query = "ops"

        Text("Base")
          .background {
            RoundedRectangle(cornerRadius: 8).fill(.blue)
          }
          .overlay(alignment: .topTrailing) {
            Text("Badge")
          }
          .safeAreaInset(edge: .bottom) {
            Text("Inset")
          }
          .contextMenu {
            Button("Open") { cmux("workspace.select", workspace_id: selectedId) }
          }
          .refreshable {
            cmux("sidebar.reload", name: sourceName)
          }
          .swipeActions(edge: .leading, allowsFullSwipe: false) {
            Button("Pin") { cmux("workspace.select", workspace_id: selectedId) }
            Button("Remove", role: .destructive) { cmux("sidebar.reload", name: sourceName) }
          }
          .searchable(text: $query, placement: .sidebar, prompt: "Filter")
          .accessibilityRepresentation {
            Label("Voice row", systemImage: "speaker.wave.2")
          }
      `,
      {
        sourceName: "ops.swift",
        workspaceCount: 0,
        selectedTitle: "Ops",
        selectedId: "workspace-a",
        unreadTotal: 7,
        portTotal: 0,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("modified");
    if (document.root.kind === "modified") {
      expect(document.root.base).toEqual({ kind: "text", text: "Base" });
      expect(document.root.childModifiers).toMatchObject([
        {
          name: "background",
          children: [{ kind: "shape", shape: "roundedRectangle", radius: 8 }],
        },
        {
          name: "overlay",
          value: "topTrailing",
          children: [{ kind: "text", text: "Badge" }],
        },
        {
          name: "safeAreaInset",
          edge: "bottom",
          children: [{ kind: "text", text: "Inset" }],
        },
        {
          name: "contextMenu",
          children: [
            {
              kind: "button",
              text: "Open",
              children: [],
              action: {
                method: "workspace.select",
                params: { workspace_id: "workspace-a" },
              },
            },
          ],
        },
        {
          name: "refreshable",
          action: {
            method: "sidebar.reload",
            params: { name: "ops.swift" },
          },
        },
        {
          name: "swipeActions",
          value: "leading",
          boolValue: false,
          children: [
            {
              kind: "button",
              text: "Pin",
              action: {
                method: "workspace.select",
                params: { workspace_id: "workspace-a" },
              },
            },
            {
              kind: "button",
              text: "Remove",
              role: "destructive",
              action: {
                method: "sidebar.reload",
                params: { name: "ops.swift" },
              },
            },
          ],
        },
        {
          name: "searchable",
          value: "ops",
          secondaryValue: "Filter",
          placement: "sidebar",
          stateBindingKey: "query",
        },
        {
          name: "accessibilityRepresentation",
          boolValue: true,
          children: [{ kind: "label", text: "Voice row", systemImage: "speaker.wave.2" }],
        },
      ]);
    }
  });

  test("parses SwiftUI local presentation modifiers", () => {
    const document = parseCustomSidebarSwift(
      `
        @State var showSheet = true
        @State var showPopover = true
        @State var showAlert = true
        @State var selectedPanel = "Details"
        @State var selectedWarning = "Build failed"
        @State var selectedAction = "Deploy"

        Text("Base")
          .sheet(isPresented: $showSheet) {
            Text("Sheet body")
              .presentationDetents([.medium, .large])
              .presentationDragIndicator(.visible)
              .presentationBackground(.regularMaterial)
              .presentationCornerRadius(22)
          }
          .sheet(item: $selectedPanel) { panel in
            Text(panel)
            Button("Done") { dismiss() }
          }
          .popover(isPresented: $showPopover) {
            Text("Popover body")
          }
          .alert("Heads up", isPresented: $showAlert) {
            Text("Alert body")
            Button("Delete", role: .destructive) { dismiss() }
            Button("Cancel", role: .cancel) { dismiss() }
          }
          .alert("Warning", item: $selectedWarning) { warning in
            Text(warning)
            Button("Acknowledge", role: .cancel) { dismiss() }
          }
          .confirmationDialog("Action menu", item: $selectedAction) { action in
            Text(action)
            Button("Run") { dismiss() }
          }
      `,
      {
        sourceName: "ops.swift",
        workspaceCount: 0,
        selectedTitle: "Ops",
        selectedId: "workspace-a",
        unreadTotal: 7,
        portTotal: 0,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("modified");
    if (document.root.kind === "modified") {
      expect(document.root.base).toEqual({ kind: "text", text: "Base" });
      expect(document.root.childModifiers).toMatchObject([
        {
          name: "sheet",
          boolValue: true,
          stateBindingKey: "showSheet",
          presentationBindingKind: "isPresented",
          children: [
            {
              kind: "text",
              text: "Sheet body",
              modifiers: [
                { name: "presentationDetents", value: "medium,large" },
                { name: "presentationDragIndicator", value: "visible" },
                { name: "presentationBackground", value: "regularMaterial" },
                { name: "presentationCornerRadius", value: "22" },
              ],
            },
          ],
        },
        {
          name: "sheet",
          boolValue: true,
          stateBindingKey: "selectedPanel",
          presentationBindingKind: "item",
          itemParam: "panel",
          itemValue: "Details",
          children: [
            { kind: "text", text: "Details" },
            { kind: "button", text: "Done", localAction: "dismissPresentation" },
          ],
        },
        {
          name: "popover",
          boolValue: true,
          stateBindingKey: "showPopover",
          children: [{ kind: "text", text: "Popover body" }],
        },
        {
          name: "alert",
          value: "Heads up",
          boolValue: true,
          stateBindingKey: "showAlert",
          children: [
            { kind: "text", text: "Alert body" },
            {
              kind: "button",
              role: "destructive",
              text: "Delete",
              localAction: "dismissPresentation",
            },
            {
              kind: "button",
              role: "cancel",
              text: "Cancel",
              localAction: "dismissPresentation",
            },
          ],
        },
        {
          name: "alert",
          value: "Warning",
          boolValue: true,
          stateBindingKey: "selectedWarning",
          presentationBindingKind: "item",
          itemParam: "warning",
          itemValue: "Build failed",
          children: [
            { kind: "text", text: "Build failed" },
            {
              kind: "button",
              role: "cancel",
              text: "Acknowledge",
              localAction: "dismissPresentation",
            },
          ],
        },
        {
          name: "confirmationDialog",
          value: "Action menu",
          boolValue: true,
          stateBindingKey: "selectedAction",
          presentationBindingKind: "item",
          itemParam: "action",
          itemValue: "Deploy",
          children: [
            { kind: "text", text: "Deploy" },
            {
              kind: "button",
              text: "Run",
              localAction: "dismissPresentation",
            },
          ],
        },
      ]);
    }
  });

  test("parses SwiftUI static navigation, toolbar, and shortcut modifiers", () => {
    const document = parseCustomSidebarSwift(
      `
        VStack {
          Text("Body")
        }
          .navigationTitle("Board")
          .navigationSubtitle(selectedTitle)
          .navigationBarTitleDisplayMode(.inline)
          .toolbarBackground(.blue, for: .navigationBar)
          .toolbarColorScheme(.dark, for: .navigationBar)
          .toolbar {
            ToolbarItem(placement: .primaryAction) {
              Button("Refresh") { cmux("sidebar.reload", name: sourceName) }
            }
          }
          .keyboardShortcut("b", modifiers: [.command, .shift])
          .contentShape(Capsule())
          .draggable(selectedId)
          .dropDestination(for: String.self) { items, location in
            cmux("sidebar.reload", name: sourceName)
          }
      `,
      {
        sourceName: "ops.swift",
        workspaceCount: 0,
        selectedTitle: "Ops",
        selectedId: "workspace-a",
        unreadTotal: 0,
        portTotal: 0,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("modified");
    if (document.root.kind === "modified") {
      expect(document.root.base.kind).toBe("vstack");
      expect(document.root.base.modifiers).toEqual([
        { name: "navigationTitle", value: "Board" },
        { name: "navigationSubtitle", value: "Ops" },
        { name: "navigationBarTitleDisplayMode", value: "inline" },
        { name: "toolbarBackground", value: "blue", secondaryValue: "navigationBar" },
        { name: "toolbarColorScheme", value: "dark", secondaryValue: "navigationBar" },
        { name: "keyboardShortcut", value: "b", secondaryValue: "command,shift" },
        { name: "contentShape", value: "capsule" },
        { name: "draggable", value: "workspace-a" },
        {
          name: "dropDestination",
          value: "String",
          action: {
            method: "sidebar.reload",
            params: { name: "ops.swift" },
          },
        },
      ]);
      expect(document.root.childModifiers).toMatchObject([
        {
          name: "toolbar",
          children: [
            {
              kind: "group",
              groupRole: "toolbarItem",
              toolbarPlacement: "primaryAction",
              children: [
                {
                  kind: "button",
                  text: "Refresh",
                  action: {
                    method: "sidebar.reload",
                    params: { name: "ops.swift" },
                  },
                },
              ],
            },
          ],
        },
      ]);
    }
  });

  test("parses SwiftUI NavigationStack and destination links", () => {
    const document = parseCustomSidebarSwift(
      `
        NavigationStack {
          NavigationLink("Workspace detail") {
            VStack {
              Text(selectedTitle)
              Button("Focus") { cmux("workspace.select", workspace_id: selectedId) }
            }
          }
          NavigationLink(destination: Text("Settings")) {
            Label("Settings", systemImage: "gear")
          }
          NavigationLink(value: selectedId) {
            Label("Route detail", systemImage: "arrow.right")
          }
        }
        .navigationDestination(for: String.self) { route in
          VStack {
            Text(route)
            Text("Route destination")
          }
        }
      `,
      {
        sourceName: "ops.swift",
        workspaceCount: 0,
        selectedTitle: "Ops",
        selectedId: "workspace-a",
        unreadTotal: 0,
        portTotal: 0,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("navigationStack");
    const root = document.root.kind === "navigationStack" ? document.root : undefined;
    expect(root?.children[0]).toMatchObject({
      kind: "navigationLink",
      title: "Workspace detail",
      children: [{ kind: "text", text: "Workspace detail" }],
      destination: [
        {
          kind: "vstack",
          children: [
            { kind: "text", text: "Ops" },
            {
              kind: "button",
              text: "Focus",
              action: {
                method: "workspace.select",
                params: { workspace_id: "workspace-a" },
              },
            },
          ],
        },
      ],
    });
    expect(root?.children[1]).toMatchObject({
      kind: "navigationLink",
      title: "Open",
      children: [{ kind: "label", text: "Settings", systemImage: "gear" }],
      destination: [{ kind: "text", text: "Settings" }],
    });
    expect(root?.children[2]).toMatchObject({
      kind: "navigationLink",
      title: "Open",
      value: "workspace-a",
      children: [{ kind: "label", text: "Route detail", systemImage: "arrow.right" }],
      destination: [
        {
          kind: "vstack",
          children: [
            { kind: "text", text: "workspace-a" },
            { kind: "text", text: "Route destination" },
          ],
        },
      ],
    });
    expect(root?.modifiers).toMatchObject([
      {
        name: "navigationDestination",
        routeParam: "route",
        routeValueType: "String",
      },
    ]);
  });

  test("parses richer SwiftUI leaf modifiers into metadata", () => {
    const document = parseCustomSidebarSwift(
      `
        VStack {
          Text("Long copy")
            .italic()
            .bold(false)
            .fontWeight(.semibold)
            .fontDesign(.rounded)
            .fontWidth(.condensed)
            .dynamicTypeSize(.accessibility2)
            .monospaced()
            .monospacedDigit()
            .lineLimit(2, reservesSpace: true)
            .multilineTextAlignment(.center)
            .opacity(0.5)
            .hidden()
            .fixedSize()
            .allowsHitTesting(false)
            .help("Explains the copy")
            .accessibilityLabel("Readable copy")
            .accessibilityValue("42 unread")
            .accessibilityHint("Opens details")
            .accessibilityAddTraits([.isButton, .isHeader])
            .accessibilitySortPriority(3)
            .accessibilityAction(named: Text("Refresh")) {
              cmux("sidebar.reload", name: sourceName)
            }
            .accessibilityActivationPoint(CGPoint(x: 12, y: 24))
            .animation(.easeInOut, value: selectedId)
            .transition(.opacity)
            .contentTransition(.opacity)
            .symbolEffect(.pulse, isActive: true, value: selectedId)
            .preferredColorScheme(.dark)
            .environment(\.colorScheme, .light)
            .environment(\.layoutDirection, .rightToLeft)
            .onGeometryChange(for: CGSize.self) { proxy in proxy.size }
          Button("Nope") { cmux("workspace.select", workspace_id: selectedId) }
            .disabled(true)
            .help("Disabled action")
          Text("Secret")
            .redacted(reason: .placeholder)
          Text("Private token")
            .privacySensitive()
          Text("Visible token")
            .redacted(reason: [.placeholder, .privacy])
            .unredacted()
          Text("Hidden")
            .accessibilityHidden(true)
          HStack {
            Text("Primary")
            Text("Secondary")
          }
            .accessibilityElement(children: .combine)
          Text("Calm symbol")
            .symbolEffect(.bounce, isActive: true)
            .symbolEffectsRemoved()
          Text("Hover")
            .hoverEffect(.lift, isEnabled: true)
            .defaultHoverEffect(.highlight)
          Text("No hover")
            .hoverEffect(.highlight, isEnabled: false)
          Text("Inbox")
            .badge(unreadTotal)
        }
      `,
      {
        sourceName: "ops.swift",
        workspaceCount: 0,
        selectedTitle: "Ops",
        selectedId: "workspace-a",
        unreadTotal: 7,
        portTotal: 0,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]?.modifiers).toEqual([
      { name: "italic" },
      { name: "bold", boolValue: false },
      { name: "fontWeight", value: "semibold" },
      { name: "fontDesign", value: "rounded" },
      { name: "fontWidth", value: "condensed" },
      { name: "dynamicTypeSize", value: "accessibility2" },
      { name: "monospaced" },
      { name: "monospacedDigit" },
      { name: "lineLimit", value: "2", boolValue: true },
      { name: "multilineTextAlignment", value: "center" },
      { name: "opacity", value: "0.5" },
      { name: "hidden", boolValue: true },
      { name: "fixedSize" },
      { name: "allowsHitTesting", boolValue: false },
      { name: "help", value: "Explains the copy" },
      { name: "accessibilityLabel", value: "Readable copy" },
      { name: "accessibilityValue", value: "42 unread" },
      { name: "accessibilityHint", value: "Opens details" },
      { name: "accessibilityAddTraits", value: "isButton,isHeader" },
      { name: "accessibilitySortPriority", value: "3" },
      {
        name: "accessibilityAction",
        value: "Refresh",
        action: { method: "sidebar.reload", params: { name: "ops.swift" } },
        boolValue: true,
      },
      { name: "accessibilityActivationPoint", value: undefined, x: 12, y: 24 },
      { name: "animation", value: "easeInOut", secondaryValue: "selectedId" },
      { name: "transition", value: "opacity" },
      { name: "contentTransition", value: "opacity" },
      {
        name: "symbolEffect",
        value: "pulse",
        boolValue: true,
        secondaryValue: "selectedId",
      },
      { name: "preferredColorScheme", value: "dark" },
      { name: "environment", value: "colorScheme", secondaryValue: "light" },
      { name: "environment", value: "layoutDirection", secondaryValue: "rightToLeft" },
      { name: "onGeometryChange", value: "CGSize", boolValue: true },
    ]);
    expect(root?.children[1]?.modifiers).toEqual([
      { name: "disabled", boolValue: true },
      { name: "help", value: "Disabled action" },
    ]);
    expect(root?.children[2]?.modifiers).toEqual([
      { name: "redacted", value: ".placeholder" },
    ]);
    expect(root?.children[3]?.modifiers).toEqual([
      { name: "privacySensitive" },
    ]);
    expect(root?.children[4]?.modifiers).toEqual([
      { name: "redacted", value: "[.placeholder, .privacy]" },
      { name: "unredacted" },
    ]);
    expect(root?.children[5]?.modifiers).toEqual([
      { name: "accessibilityHidden", boolValue: true },
    ]);
    expect(root?.children[6]?.modifiers).toEqual([
      { name: "accessibilityElement", value: "combine" },
    ]);
    expect(root?.children[7]?.modifiers).toEqual([
      { name: "symbolEffect", value: "bounce", boolValue: true, secondaryValue: undefined },
      { name: "symbolEffectsRemoved", boolValue: true },
    ]);
    expect(root?.children[8]?.modifiers).toEqual([
      { name: "hoverEffect", value: "lift", boolValue: true },
      { name: "defaultHoverEffect", value: "highlight" },
    ]);
    expect(root?.children[9]?.modifiers).toEqual([
      { name: "hoverEffect", value: "highlight", boolValue: false },
    ]);
    expect(root?.children[10]?.modifiers).toEqual([
      { name: "badge", value: "7" },
    ]);
  });

  test("parses text, list, scroll, and symbol presentation modifiers", () => {
    const document = parseCustomSidebarSwift(
      `
        List {
          Text("stale branch")
            .truncationMode(.tail)
            .textCase(.uppercase)
            .underline(true, pattern: .dash, color: .mint)
            .strikethrough(true, pattern: .dot, color: .red)
          Image(systemName: "bolt.fill")
            .resizable(capInsets: EdgeInsets(top: 2, leading: 4, bottom: 6, trailing: 8), resizingMode: .tile)
            .scaledToFit()
            .renderingMode(.template)
            .interpolation(.none)
            .antialiased(false)
            .flipsForRightToLeftLayoutDirection(true)
            .imageScale(.large)
            .symbolRenderingMode(.hierarchical)
            .symbolVariant(.fill)
          Image("product.logo")
            .resizable()
          Image(decorative: "badge.icon")
          AsyncImage(url: URL(string: "https://example.com/avatar.png"))
            .resizable()
            .scaledToFill()
          AsyncImage(
            url: URL(string: "https://example.com/hero.png"),
            content: { image in
              image
                .resizable()
                .scaledToFill()
            },
            placeholder: {
              ProgressView(value: 0.25)
            }
          )
          AsyncImage(url: URL(string: "https://example.com/trailing.png")) { image in
            image
              .resizable()
          } placeholder: {
            Text("Loading trailing")
          }
            .frame(width: 32, height: 24)
          Text("SHOUT")
            .truncationMode(.head)
            .textCase(.lowercase)
            .multilineTextAlignment(.trailing)
          Text("middle cut")
            .truncationMode(.middle)
        }
          .listStyle(.sidebar)
          .scrollContentBackground(.hidden)
          .scrollIndicators(.hidden, axes: .vertical)
          .scrollClipDisabled()
          .scrollTargetBehavior(.paging)
          .scrollTargetLayout()
          .scrollBounceBehavior(.basedOnSize, axes: .vertical)
          .scrollDisabled(false)
          .scrollPosition(id: selectedId, anchor: .center)
          .defaultScrollAnchor(.bottom)
      `,
      {
        sourceName: "ops.swift",
        workspaceCount: 0,
        selectedTitle: "Ops",
        selectedId: "workspace-a",
        unreadTotal: 0,
        portTotal: 0,
        assets: {
          "product.logo": "https://example.com/product.png",
          "badge.icon": "cmux-sidebar-asset://ops/badge.svg",
        },
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("list");
    const list = document.root.kind === "list" ? document.root : undefined;
    expect(list?.modifiers).toEqual([
      { name: "listStyle", value: "sidebar" },
      { name: "scrollContentBackground", value: "hidden" },
      { name: "scrollIndicators", value: "hidden", secondaryValue: "vertical" },
      { name: "scrollClipDisabled", boolValue: true },
      { name: "scrollTargetBehavior", value: "paging" },
      { name: "scrollTargetLayout", boolValue: true },
      { name: "scrollBounceBehavior", value: "basedOnSize", secondaryValue: "vertical" },
      { name: "scrollDisabled", boolValue: false },
      { name: "scrollPosition", value: "workspace-a", secondaryValue: "center" },
      { name: "defaultScrollAnchor", value: "bottom" },
    ]);
    expect(list?.children[0]?.modifiers).toEqual([
      { name: "truncationMode", value: "tail" },
      { name: "textCase", value: "uppercase" },
      { name: "underline", boolValue: true, value: "mint", secondaryValue: "dash" },
      { name: "strikethrough", boolValue: true, value: "red", secondaryValue: "dot" },
    ]);
    expect(list?.children[1]?.modifiers).toEqual([
      {
        name: "resizable",
        secondaryValue: "tile",
        capInsetTop: 2,
        capInsetLeading: 4,
        capInsetBottom: 6,
        capInsetTrailing: 8,
      },
      { name: "aspectRatio", secondaryValue: "fit" },
      { name: "renderingMode", value: "template" },
      { name: "interpolation", value: "none" },
      { name: "antialiased", boolValue: false },
      { name: "flipsForRightToLeftLayoutDirection", boolValue: true },
      { name: "imageScale", value: "large" },
      { name: "symbolRenderingMode", value: "hierarchical" },
      { name: "symbolVariant", value: "fill" },
    ]);
    expect(list?.children[2]).toEqual({
      kind: "assetImage",
      name: "product.logo",
      url: "https://example.com/product.png",
      decorative: false,
      modifiers: [{ name: "resizable" }],
    });
    expect(list?.children[3]).toEqual({
      kind: "assetImage",
      name: "badge.icon",
      url: "cmux-sidebar-asset://ops/badge.svg",
      decorative: true,
    });
    expect(list?.children[4]).toEqual({
      kind: "asyncImage",
      url: "https://example.com/avatar.png",
      modifiers: [{ name: "resizable" }, { name: "aspectRatio", secondaryValue: "fill" }],
    });
    expect(list?.children[5]).toEqual({
      kind: "asyncImage",
      url: "https://example.com/hero.png",
      successChildren: [
        {
          kind: "asyncImage",
          url: "https://example.com/hero.png",
          modifiers: [
            { name: "resizable", secondaryValue: undefined },
            { name: "aspectRatio", secondaryValue: "fill" },
          ],
        },
      ],
      placeholderChildren: [{ kind: "progress", value: 0.25, total: 1 }],
    });
    expect(list?.children[6]).toEqual({
      kind: "asyncImage",
      url: "https://example.com/trailing.png",
      successChildren: [
        {
          kind: "asyncImage",
          url: "https://example.com/trailing.png",
          modifiers: [{ name: "resizable", secondaryValue: undefined }],
        },
      ],
      placeholderChildren: [{ kind: "text", text: "Loading trailing" }],
      modifiers: [
        {
          name: "frame",
          frameWidth: 32,
          frameHeight: 24,
          frameMinWidth: undefined,
          frameMinHeight: undefined,
          frameMaxWidth: undefined,
          frameMaxHeight: undefined,
          frameIdealWidth: undefined,
          frameIdealHeight: undefined,
          frameAlignment: undefined,
          maxWidthInfinity: false,
        },
      ],
    });
    expect(list?.children[7]?.modifiers).toEqual([
      { name: "truncationMode", value: "head" },
      { name: "textCase", value: "lowercase" },
      { name: "multilineTextAlignment", value: "trailing" },
    ]);
    expect(list?.children[8]?.modifiers).toEqual([
      { name: "truncationMode", value: "middle" },
    ]);
  });

  test("parses built-in control style modifiers", () => {
    const document = parseCustomSidebarSwift(
      `
        VStack {
          Label("Repo", systemImage: "folder")
            .labelStyle(.iconOnly)
          Button("Run") { cmux("workspace.select", workspace_id: selectedId) }
            .controlSize(.small)
            .buttonStyle(.borderedProminent)
            .buttonBorderShape(.capsule)
          Toggle("Pinned", isOn: .constant(true))
            .toggleStyle(.button)
          TextField("Title", text: .constant(selectedTitle))
            .textFieldStyle(.roundedBorder)
            .labelsHidden()
          Picker("Lane", selection: .constant(selectedId)) {
            Text(selectedTitle)
          }
            .pickerStyle(.segmented)
          Menu("Actions") {
            Button("Reload") { cmux("sidebar.reload", name: sourceName) }
          }
            .menuStyle(.button)
        }
      `,
      {
        sourceName: "ops.swift",
        workspaceCount: 0,
        selectedTitle: "Ops",
        selectedId: "workspace-a",
        unreadTotal: 0,
        portTotal: 0,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]?.modifiers).toEqual([
      { name: "labelStyle", value: "iconOnly" },
    ]);
    expect(root?.children[1]?.modifiers).toEqual([
      { name: "controlSize", value: "small" },
      { name: "buttonStyle", value: "borderedProminent" },
      { name: "buttonBorderShape", value: "capsule" },
    ]);
    expect(root?.children[2]?.modifiers).toEqual([
      { name: "toggleStyle", value: "button" },
    ]);
    expect(root?.children[3]?.modifiers).toEqual([
      { name: "textFieldStyle", value: "roundedBorder" },
      { name: "labelsHidden", boolValue: true },
    ]);
    expect(root?.children[4]?.modifiers).toEqual([
      { name: "pickerStyle", value: "segmented" },
    ]);
    expect(root?.children[5]?.modifiers).toEqual([
      { name: "menuStyle", value: "button" },
    ]);
  });

  test("parses layout and decoration modifiers into safe metadata", () => {
    const document = parseCustomSidebarSwift(
      `
        VStack {
          Text("Badge")
            .layoutPriority(2)
            .offset(x: 4, y: -2)
            .position(x: 12, y: 24)
            .zIndex(3)
            .aspectRatio(1.5, contentMode: .fit)
            .clipped()
            .compositingGroup()
            .clipShape(Capsule(), style: FillStyle(eoFill: true, antialiased: false))
            .shadow(color: .black, radius: 6, x: 1, y: 2)
            .border(.gray, width: 2)
            .blur(radius: 1)
            .brightness(0.1)
            .contrast(1.2)
            .saturation(0.8)
            .grayscale(0.25)
            .hueRotation(Angle.radians(1.5707963267948966))
            .blendMode(.screen)
            .rotationEffect(Angle(degrees: 15))
            .scaleEffect(1.1)
            .rotation3DEffect(Angle.degrees(30), axis: (x: 0, y: 1, z: 0), anchor: UnitPoint(x: 0, y: 0), perspective: 0.7)
          Circle()
            .fill(.green)
            .stroke(.blue, width: 2)
          Ellipse()
            .trim(from: 0.25, to: 0.75)
          UnevenRoundedRectangle()
            .stroke(.mint, width: 3)
          RoundedRectangle(cornerRadius: 10)
            .strokeBorder(.orange, lineWidth: 4)
          Text("Gradient")
            .background(LinearGradient(colors: [.red, .blue], startPoint: .topLeading, endPoint: .bottomTrailing))
          Rectangle()
            .fill(RadialGradient(colors: [.teal, .black], center: .center, startRadius: 0, endRadius: 24))
        }
      `,
      {
        sourceName: "ops.swift",
        workspaceCount: 0,
        selectedTitle: "Ops",
        selectedId: "workspace-a",
        unreadTotal: 0,
        portTotal: 0,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]?.modifiers).toEqual([
      { name: "layoutPriority", value: "2" },
      { name: "offset", x: 4, y: -2 },
      { name: "position", x: 12, y: 24 },
      { name: "zIndex", value: "3" },
      { name: "aspectRatio", value: "1.5", secondaryValue: "fit" },
      { name: "clipped" },
      { name: "compositingGroup" },
      { name: "clipShape", value: "capsule", fillStyle: "eoFill", antialiased: false },
      { name: "shadow", value: "black", radius: 6, x: 1, y: 2 },
      { name: "border", value: "gray", width: 2 },
      { name: "blur", radius: 1 },
      { name: "brightness", value: "0.1" },
      { name: "contrast", value: "1.2" },
      { name: "saturation", value: "0.8" },
      { name: "grayscale", value: "0.25" },
      { name: "hueRotation", value: "90" },
      { name: "blendMode", value: "screen" },
      { name: "rotationEffect", value: "15" },
      { name: "scaleEffect", value: "1.1" },
      {
        name: "rotation3DEffect",
        value: "30",
        secondaryValue: "topLeading",
        x: 0,
        y: 1,
        z: 0,
        perspective: 0.7,
      },
    ]);
    expect(root?.children[1]?.modifiers).toEqual([
      { name: "foregroundColor", value: "green" },
      { name: "border", value: "blue", width: 2 },
    ]);
    expect(root?.children[2]).toEqual({
      kind: "shape",
      shape: "ellipse",
      modifiers: [{ name: "trim", x: 0.25, y: 0.75 }],
    });
    expect(root?.children[3]).toEqual({
      kind: "shape",
      shape: "unevenRoundedRectangle",
      modifiers: [{ name: "border", value: "mint", width: 3 }],
    });
    expect(root?.children[4]).toEqual({
      kind: "shape",
      shape: "roundedRectangle",
      radius: 10,
      modifiers: [{ name: "strokeBorder", value: "orange", width: 4 }],
    });
    expect(root?.children[5]?.modifiers).toEqual([
      {
        name: "background",
        value:
          "LinearGradient(colors: [.red, .blue], startPoint: .topLeading, endPoint: .bottomTrailing)",
      },
    ]);
    expect(root?.children[6]?.modifiers).toEqual([
      {
        name: "foregroundColor",
        value:
          "RadialGradient(colors: [.teal, .black], center: .center, startRadius: 0, endRadius: 24)",
      },
    ]);
  });

  test("parses Swift array helper loops and enumerated pairs", () => {
    const document = parseCustomSidebarSwift(
      `
        VStack {
          ForEach(Array(workspaces.enumerated()), id: \\.offset) { index, workspace in
            Text("\\(index): \\(workspace.title)")
          }
          ForEach(workspaces.dropFirst().prefix(1)) { workspace in
            Text("Next \\(workspace.title)")
          }
          for workspace in workspaces.reversed().suffix(1) {
            Text("Tail \\(workspace.title)")
          }
        }
      `,
      {
        sourceName: "ops.swift",
        workspaceCount: 3,
        selectedTitle: "Ops",
        selectedId: "workspace-a",
        unreadTotal: 0,
        portTotal: 0,
      },
      [
        workspacePreview({
          id: "workspace-a",
          index: 0,
          title: "Ops lane",
          selected: true,
        }),
        workspacePreview({ id: "workspace-b", index: 1, title: "API lane" }),
        workspacePreview({ id: "workspace-c", index: 2, title: "Docs lane" }),
      ],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children).toEqual([
      { kind: "text", text: "0: Ops lane", modifiers: [{ name: "id", value: "0" }] },
      { kind: "text", text: "1: API lane", modifiers: [{ name: "id", value: "1" }] },
      { kind: "text", text: "2: Docs lane", modifiers: [{ name: "id", value: "2" }] },
      { kind: "text", text: "Next API lane" },
      { kind: "text", text: "Tail Ops lane" },
    ]);
  });

  test("parses scroll views, lazy stack metadata, button roles, and list row modifiers", () => {
    const document = parseCustomSidebarSwift(
      `
        ScrollView(.horizontal, showsIndicators: false) {
          LazyHStack(alignment: .top, spacing: 14, pinnedViews: [.sectionHeaders]) {
            Button("Delete", role: .destructive) {
              cmux("workspace.set_status", key: "delete", value: "armed")
            }
              .listRowBackground(.blue)
              .listRowSeparator(.hidden)
            Button("Cancel", role: .cancel) {
              cmux("workspace.clear_status", key: "delete")
            }
          }
        }
        ScrollView([.horizontal, .vertical], showsIndicators: true) {
          Text("Pan")
        }
      `,
      {
        sourceName: "ops.swift",
        workspaceCount: 0,
        selectedTitle: "Ops",
        selectedId: "workspace-a",
        unreadTotal: 0,
        portTotal: 0,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("group");
    const root = document.root.kind === "group" ? document.root : undefined;
    expect(root?.children[0]).toMatchObject({
      kind: "scrollView",
      axis: "horizontal",
      showsIndicators: false,
    });
    expect(root?.children[1]).toMatchObject({
      kind: "scrollView",
      axis: "both",
      showsIndicators: true,
      children: [{ kind: "text", text: "Pan" }],
    });
    const scroll = root?.children[0]?.kind === "scrollView" ? root.children[0] : undefined;
    expect(scroll?.children[0]).toMatchObject({
      kind: "hstack",
      alignment: "top",
      spacing: 14,
      lazyStack: true,
      pinnedViews: "sectionHeaders",
    });
    const row = scroll?.children[0]?.kind === "hstack" ? scroll.children[0] : undefined;
    expect(row?.children[0]).toMatchObject({
      kind: "button",
      role: "destructive",
      text: "Delete",
    });
    expect(row?.children[0]?.modifiers).toEqual([
      { name: "listRowBackground", value: "blue" },
      { name: "listRowSeparator", value: "hidden" },
    ]);
    expect(row?.children[1]).toMatchObject({
      kind: "button",
      role: "cancel",
      text: "Cancel",
    });
  });

  test("parses Swift split views as safe split containers", () => {
    const document = parseCustomSidebarSwift(
      `
        HSplitView {
          VStack { Text("Left") }
          VSplitView {
            Text("Top")
            Text("Bottom")
          }
        }
      `,
      {
        sourceName: "ops.swift",
        workspaceCount: 0,
        selectedTitle: "Ops",
        selectedId: "workspace-a",
        unreadTotal: 0,
        portTotal: 0,
      },
      [],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root).toMatchObject({
      kind: "splitView",
      axis: "horizontal",
    });
    const split = document.root.kind === "splitView" ? document.root : undefined;
    expect(split?.children[0]).toMatchObject({ kind: "vstack" });
    expect(split?.children[1]).toMatchObject({
      kind: "splitView",
      axis: "vertical",
    });
  });

  test("parses if-let, shapes, grids, zstacks, and menus", () => {
    const document = parseCustomSidebarSwift(
      `
        VStack {
          let columns = [
            GridItem(.fixed(32)),
            GridItem(.flexible(minimum: 48, maximum: 96)),
            GridItem(.adaptive(minimum: 40), spacing: 4, alignment: .center)
          ]
          if let branch = workspaces[0].branch {
            Text(branch)
          } else {
            Text("missing")
          }
          if let remote = workspaces[1].remoteState {
            Text(remote)
          } else {
            Text("local")
          }
          Grid {
            GridRow {
              Circle().foregroundColor(.green).gridCellColumns(2).gridColumnAlignment(.trailing).gridCellAnchor(.bottomTrailing)
              RoundedRectangle(cornerRadius: 6, style: .continuous).foregroundColor(.blue)
              Capsule(style: .circular).foregroundColor(.orange)
              Rectangle().foregroundColor(.gray)
              UnevenRoundedRectangle().foregroundColor(.mint)
              Ellipse().trim(from: 0.2, to: 0.8)
              ContainerRelativeShape().foregroundColor(.teal)
              Path(roundedRect: CGRect(x: 0, y: 0, width: 24, height: 12), cornerRadius: 4).fill(.orange)
              Path(ellipseIn: CGRect(origin: CGPoint.zero, size: CGSize(width: 18, height: 10))).fill(.pink)
            }
          }
          LazyVGrid(columns: columns, spacing: 8) {
            Text("One")
            Text("Two")
          }
          ZStack {
            Circle().foregroundColor(.teal)
            Text("Z").font(.caption).containerRelativeFrame(.horizontal, count: 4, span: 2, spacing: 8, alignment: .center).alignmentGuide(.leading) { _ in 12 }.visualEffect { content, proxy in content }
          }
            .coordinateSpace(name: "board")
          Menu("Actions") {
            Button("Select") { cmux("workspace.select", workspace_id: workspaces[0].id) }
          }
          ControlGroup {
            Button("Run") { cmux("sidebar.reload", name: sourceName) }
            Button("Stop") { cmux("sidebar.reload", name: sourceName) }
          }
            .controlGroupStyle(.compactMenu)
        }
      `,
      {
        sourceName: "ops.swift",
        workspaceCount: 2,
        selectedTitle: "Ops lane",
        selectedId: "workspace-a",
        unreadTotal: 0,
        portTotal: 0,
      },
      [
        {
          id: "workspace-a",
          index: 0,
          title: "Ops lane",
          branch: "feature/grid",
          tabs: [],
          tabCount: 0,
          unreadCount: 0,
          ports: [],
          dirty: false,
          statusCount: 0,
          metadataCount: 0,
          logCount: 0,
          selected: true,
        },
        {
          id: "workspace-b",
          index: 1,
          title: "API lane",
          tabs: [],
          tabCount: 0,
          unreadCount: 0,
          ports: [],
          dirty: false,
          statusCount: 0,
          metadataCount: 0,
          logCount: 0,
          selected: false,
        },
      ],
    );

    expect(document.warnings).toEqual([]);
    expect(document.root.kind).toBe("vstack");
    const root = document.root.kind === "vstack" ? document.root : undefined;
    expect(root?.children[0]).toEqual({ kind: "text", text: "feature/grid" });
    expect(root?.children[1]).toEqual({ kind: "text", text: "local" });
    expect(root?.children[2]).toMatchObject({ kind: "grid", gridKind: "grid" });
    const grid = root?.children[2]?.kind === "grid" ? root.children[2] : undefined;
    expect(grid?.children[0]?.kind).toBe("gridRow");
    const row = grid?.children[0]?.kind === "gridRow" ? grid.children[0] : undefined;
    expect(row?.children.map((node) => node.kind)).toEqual([
      "shape",
      "shape",
      "shape",
      "shape",
      "shape",
      "shape",
      "shape",
      "shape",
      "shape",
    ]);
    expect(row?.children[6]).toMatchObject({
      kind: "shape",
      shape: "containerRelativeShape",
    });
    expect(row?.children[1]).toMatchObject({
      kind: "shape",
      shape: "roundedRectangle",
      radius: 6,
      cornerStyle: "continuous",
    });
    expect(row?.children[2]).toMatchObject({
      kind: "shape",
      shape: "capsule",
      cornerStyle: "circular",
    });
    expect(row?.children[7]).toMatchObject({
      kind: "shape",
      shape: "pathRoundedRect",
      radius: 4,
      pathX: 0,
      pathY: 0,
      pathWidth: 24,
      pathHeight: 12,
      modifiers: [{ name: "foregroundColor", value: "orange" }],
    });
    expect(row?.children[8]).toMatchObject({
      kind: "shape",
      shape: "pathEllipse",
      pathX: 0,
      pathY: 0,
      pathWidth: 18,
      pathHeight: 10,
      modifiers: [{ name: "foregroundColor", value: "pink" }],
    });
    expect(row?.children[0]?.modifiers).toEqual([
      { name: "foregroundColor", value: "green" },
      { name: "gridCellColumns", value: "2" },
      { name: "gridColumnAlignment", value: "trailing" },
      { name: "gridCellAnchor", value: "bottomTrailing" },
    ]);
    expect(root?.children[3]).toMatchObject({
      kind: "grid",
      gridKind: "lazyVGrid",
      spacing: 8,
      gridItems: [
        { size: "fixed", minimum: 32 },
        { size: "flexible", minimum: 48, maximum: 96 },
        { size: "adaptive", minimum: 40, spacing: 4, alignment: "center" },
      ],
    });
    expect(root?.children[4]?.kind).toBe("zstack");
    if (root?.children[4]?.kind === "zstack") {
      expect(root.children[4].modifiers).toEqual([
        { name: "coordinateSpace", value: "board" },
      ]);
      expect(root.children[4].children[1]?.modifiers).toEqual([
        { name: "font", value: "caption" },
        {
          name: "containerRelativeFrame",
          value: "horizontal",
          count: 4,
          span: 2,
          spacing: 8,
          frameAlignment: "center",
        },
        { name: "alignmentGuide", value: "leading", secondaryValue: "12" },
        { name: "visualEffect", boolValue: true },
      ]);
    }
    expect(root?.children[5]).toMatchObject({
      kind: "menu",
      title: "Actions",
    });
    expect(root?.children[6]).toEqual({
      kind: "group",
      groupRole: "controlGroup",
      modifiers: [{ name: "controlGroupStyle", value: "compactMenu" }],
      children: [
        {
          kind: "button",
          text: "Run",
          action: { method: "sidebar.reload", params: { name: "ops.swift" } },
          role: undefined,
          children: [],
        },
        {
          kind: "button",
          text: "Stop",
          action: { method: "sidebar.reload", params: { name: "ops.swift" } },
          role: undefined,
          children: [],
        },
      ],
    });
  });

  test("renders expanded authored Swift subset against live session data", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [
      workspace({
        workspace_id: "workspace-a",
        custom_title: "Ops lane",
        sidebar_progress: { value: 0.25, label: "Quarter" },
      }),
      workspace({
        workspace_id: "workspace-b",
        custom_title: "API lane",
      }),
    ];
    const source = `
      List {
        Section("Repos") {
          Label(selectedTitle, systemImage: "folder")
          Label(title: { Text("Route") }, icon: { Image(systemName: "arrow.right") })
          ProgressView(value: workspaces[0].progress, total: 1)
          ForEach(workspaces.indices) { i in
            HStack {
              Image(systemName: workspaces[i].selected ? "checkmark.circle" : "circle")
              Text(workspaces[i].title)
            }
          }
          for n in 0..<workspaceCount {
            Text("slot \\(n)")
          }
        }
        Section(header: Text("Summary"), footer: Text("Updated live")) {
          Text(selectedTitle)
        }
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\expanded.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("cmux-custom-sidebar-swift-list");
    expect(markup).toContain("cmux-custom-sidebar-swift-section");
    expect(markup).toContain("cmux-custom-sidebar-swift-section-header");
    expect(markup).toContain("cmux-custom-sidebar-swift-section-footer");
    expect(markup).toContain("Repos");
    expect(markup).toContain("Summary");
    expect(markup).toContain("Updated live");
    expect(markup).toContain("Ops lane");
    expect(markup).toContain("Route");
    expect(markup).toContain("aria-label=\"arrow.right\"");
    expect(markup).toContain('data-swift-system-image="folder"');
    expect(markup).toContain('data-swift-system-image="arrow.right"');
    expect(markup).toContain('data-swift-system-image="checkmark.circle"');
    expect(markup).toContain('data-swift-system-image-glyph="check"');
    expect(markup).toContain('data-swift-system-image-glyph="arrow"');
    expect(markup).toContain("API lane");
    expect(markup).toContain("role=\"progressbar\"");
    expect(markup).toContain("slot 0");
    expect(markup).toContain("slot 1");
    expect(markup).toContain("Windows/Tauri Swift subset renderer");

    const dataListMarkup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\data-list.swift"
        sourceOverride={`
          List(workspaces, id: \\.id) { workspace in
            Text(workspace.title)
          }
        `}
      />,
    );

    expect(dataListMarkup).toContain("cmux-custom-sidebar-swift-list-data");
    expect(dataListMarkup).toContain('data-swift-list-id="id"');
    expect(dataListMarkup).toContain("Ops lane");
    expect(dataListMarkup).toContain("API lane");
  });

  test("renders Swift Gauge, AnyView, and ViewThatFits wrappers", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [
      workspace({
        workspace_id: "workspace-a",
        custom_title: "Ops lane",
        sidebar_progress: { value: 0.42, label: "Almost" },
      }),
    ];
    const source = `
      VStack {
        Gauge(value: workspaces[0].progress, in: 0...100)
        AnyView(Text(selectedTitle))
        ViewThatFits(in: .horizontal) {
          Text("Compact")
          Text("Wide")
        }
        TabView {
          Text("First page")
            .tabItem { Text("Overview") }
          Text("Second page")
            .tabItem { Label("Details", systemImage: "list.bullet") }
        }
          .tabViewStyle(.page)
        Group {
          Text("Grouped")
        }
        GroupBox(label: {
          Label("Health", systemImage: "heart")
        }) {
          Text("Nominal")
        }
          .groupBoxStyle(.card)
        EmptyView()
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\wrappers.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("role=\"progressbar\"");
    expect(markup).toContain("width:42%");
    expect(markup).toContain(">Ops lane<");
    expect(markup).toContain("cmux-custom-sidebar-swift-view-that-fits");
    expect(markup).toContain("cmux-custom-sidebar-swift-view-that-fits-horizontal");
    expect(markup).toContain('data-swift-view-that-fits="horizontal"');
    expect(markup).toContain("cmux-custom-sidebar-swift-tab-view");
    expect(markup).toContain("cmux-custom-sidebar-swift-tab-view-page");
    expect(markup).toContain("cmux-custom-sidebar-swift-tab-view-style-page");
    expect(markup).toContain('data-swift-tab-view="true"');
    expect(markup).toContain('data-swift-tab-view-style="page"');
    expect(markup).toContain('data-swift-tab-view-page="0"');
    expect(markup).toContain("cmux-custom-sidebar-swift-tab-view-tabs");
    expect(markup).toContain("cmux-custom-sidebar-swift-tab-view-tab");
    expect(markup).toContain('data-swift-tab-items="true"');
    expect(markup).toContain('data-swift-tab-item-index="0"');
    expect(markup).toContain('data-swift-tab-item="Overview"');
    expect(markup).toContain('data-swift-tab-item="Details"');
    expect(markup).toContain(">First page<");
    expect(markup).toContain(">Second page<");
    expect(markup).toContain("cmux-custom-sidebar-swift-group");
    expect(markup).toContain('data-swift-group="true"');
    expect(markup).toContain("display:contents");
    expect(markup).toContain("cmux-custom-sidebar-swift-group-box");
    expect(markup).toContain("cmux-custom-sidebar-swift-group-box-style-card");
    expect(markup).toContain('data-swift-group-box="Health"');
    expect(markup).toContain('data-swift-group-box-label="Health"');
    expect(markup).toContain('data-swift-group-box-style="card"');
    expect(markup).toContain("cmux-custom-sidebar-swift-group-box-label");
    expect(markup).toContain("cmux-custom-sidebar-swift-group-box-content");
    expect(markup).toContain('data-swift-system-image="heart"');
    expect(markup).toContain(">Compact<");
    expect(markup).toContain(">Wide<");
    expect(markup).toContain(">Grouped<");
    expect(markup).toContain(">Nominal<");
    expect(markup).not.toContain("Unsupported Swift view");
  });

  test("renders Swift DisclosureGroup containers", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [
      workspace({
        workspace_id: "workspace-a",
        custom_title: "Ops lane",
      }),
    ];
    const source = `
      @State var advancedOpen = true

      VStack {
        DisclosureGroup("Ports") {
          Text("1 listening")
        }
        DisclosureGroup(isExpanded: $advancedOpen) {
          Text("Advanced controls")
        } label: {
          Label("Advanced", systemImage: "gearshape")
        }
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\disclosure.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("cmux-custom-sidebar-swift-disclosure-group");
    expect(markup).toContain("cmux-custom-sidebar-swift-disclosure-group-expanded");
    expect(markup).toContain("cmux-custom-sidebar-swift-disclosure-summary");
    expect(markup).toContain("cmux-custom-sidebar-swift-disclosure-content");
    expect(markup).toContain('data-swift-disclosure-group="Ports"');
    expect(markup).toContain('data-swift-disclosure-group-label="Advanced"');
    expect(markup).toContain('data-swift-disclosure-expanded="true"');
    expect(markup).toContain('data-swift-state-binding="advancedOpen"');
    expect(markup).toContain('data-swift-system-image="gearshape"');
    expect(markup).toContain(">1 listening<");
    expect(markup).toContain(">Advanced controls<");
    expect(markup).not.toContain("Unsupported Swift view");
  });

  test("renders if-let, shapes, grids, zstacks, and menus", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [
      workspace({
        workspace_id: "workspace-a",
        custom_title: "Ops lane",
        panel_git_branches: [
          { panel_id: "surface-1", branch: "feature/grid", is_dirty: false },
        ],
      }),
      workspace({
        workspace_id: "workspace-b",
        custom_title: "API lane",
      }),
    ];
    const source = `
      VStack {
        if let branch = workspaces[0].branch {
          Text(branch)
        }
        LabeledContent("Workspace", value: selectedTitle)
        LabeledContent("Status") {
          Text("Healthy")
          Label("Checked", systemImage: "checkmark.circle")
        }
        Grid {
          GridRow {
            Circle().foregroundColor(.green).gridCellColumns(2).gridColumnAlignment(.trailing).gridCellAnchor(.bottomTrailing)
            RoundedRectangle(cornerRadius: 6, style: .continuous).foregroundColor(.blue)
            Capsule(style: .circular).foregroundColor(.orange)
            UnevenRoundedRectangle().foregroundColor(.mint)
            Ellipse().trim(from: 0.2, to: 0.8)
            ContainerRelativeShape().foregroundColor(.teal)
            Path(roundedRect: CGRect(x: 0, y: 0, width: 24, height: 12), cornerRadius: 4).fill(.orange)
            Path(ellipseIn: CGRect(origin: CGPoint.zero, size: CGSize(width: 18, height: 10))).fill(.pink)
          }
        }
        LazyHGrid(rows: [GridItem(.fixed(28)), GridItem(.flexible(minimum: 36))], spacing: 10) {
          Text("Alpha")
          Text("Beta")
        }
        ZStack {
          Circle().foregroundColor(.teal)
          Text("Z").font(.caption).containerRelativeFrame(.horizontal, count: 4, span: 2, spacing: 8, alignment: .center).alignmentGuide(.leading) { _ in 12 }.visualEffect { content, proxy in content }
        }
          .coordinateSpace(name: "board")
        Menu("Actions") {
          Button("Select") { cmux("workspace.select", workspace_id: workspaces[0].id) }
        }
        ControlGroup {
          Button("Run") { cmux("sidebar.reload", name: sourceName) }
          Button("Stop") { cmux("sidebar.reload", name: sourceName) }
        }
          .controlGroupStyle(.compactMenu)
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\visual.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("feature/grid");
    expect(markup).toContain("cmux-custom-sidebar-swift-labeled-content");
    expect(markup).toContain('data-swift-labeled-content="Workspace"');
    expect(markup).toContain("cmux-custom-sidebar-swift-labeled-content-label");
    expect(markup).toContain("cmux-custom-sidebar-swift-labeled-content-value");
    expect(markup).toContain(">Workspace<");
    expect(markup).toContain(">Healthy<");
    expect(markup).toContain(">Checked<");
    expect(markup).toContain("cmux-custom-sidebar-swift-grid");
    expect(markup).toContain("cmux-custom-sidebar-swift-grid-grid");
    expect(markup).toContain("cmux-custom-sidebar-swift-grid-row");
    expect(markup).toContain("cmux-custom-sidebar-swift-grid-lazyHGrid");
    expect(markup).toContain("cmux-custom-sidebar-swift-grid-item-fixed");
    expect(markup).toContain("cmux-custom-sidebar-swift-grid-item-flexible");
    expect(markup).toContain('data-swift-grid-kind="lazyHGrid"');
    expect(markup).toContain('data-swift-grid-items="fixed(min:28)|flexible(min:36)"');
    expect(markup).toContain("grid-template-rows:28px minmax(36px, 1fr)");
    expect(markup).toContain("grid-auto-flow:column");
    expect(markup).toContain("cmux-custom-sidebar-swift-shape-circle");
    expect(markup).toContain("cmux-custom-sidebar-swift-grid-cell-columns-2");
    expect(markup).toContain("cmux-custom-sidebar-swift-grid-column-alignment-trailing");
    expect(markup).toContain("cmux-custom-sidebar-swift-grid-cell-anchor-bottomTrailing");
    expect(markup).toContain('data-swift-grid-cell-columns="2"');
    expect(markup).toContain('data-swift-grid-column-alignment="trailing"');
    expect(markup).toContain('data-swift-grid-cell-anchor="bottomTrailing"');
    expect(markup).toContain("grid-column:span 2");
    expect(markup).toContain("justify-self:end");
    expect(markup).toContain("place-self:end end");
    expect(markup).toContain("cmux-custom-sidebar-swift-shape-roundedRectangle");
    expect(markup).toContain("cmux-custom-sidebar-swift-shape-capsule");
    expect(markup).toContain("cmux-custom-sidebar-swift-shape-style-continuous");
    expect(markup).toContain("cmux-custom-sidebar-swift-shape-style-circular");
    expect(markup).toContain('data-swift-shape-style="continuous"');
    expect(markup).toContain('data-swift-shape-style="circular"');
    expect(markup).toContain("cmux-custom-sidebar-swift-shape-unevenRoundedRectangle");
    expect(markup).toContain("cmux-custom-sidebar-swift-shape-ellipse");
    expect(markup).toContain("cmux-custom-sidebar-swift-shape-containerRelativeShape");
    expect(markup).toContain("cmux-custom-sidebar-swift-shape-pathRoundedRect");
    expect(markup).toContain("cmux-custom-sidebar-swift-shape-pathEllipse");
    expect(markup).toContain('data-swift-path="roundedRect"');
    expect(markup).toContain('data-swift-path="ellipseIn"');
    expect(markup).toContain('data-swift-path-width="24"');
    expect(markup).toContain('data-swift-path-height="12"');
    expect(markup).toContain('data-swift-path-width="18"');
    expect(markup).toContain('data-swift-path-height="10"');
    expect(markup).toContain("width:24px");
    expect(markup).toContain("height:12px");
    expect(markup).toContain("cmux-custom-sidebar-swift-shape-trim");
    expect(markup).toContain("--cmux-custom-sidebar-swift-shape-trim-from:20%");
    expect(markup).toContain("--cmux-custom-sidebar-swift-shape-trim-to:80%");
    expect(markup).toContain("cmux-custom-sidebar-swift-zstack");
    expect(markup).toContain("cmux-custom-sidebar-swift-coordinate-space");
    expect(markup).toContain("cmux-custom-sidebar-swift-coordinate-space-board");
    expect(markup).toContain('data-swift-coordinate-space="board"');
    expect(markup).toContain("cmux-custom-sidebar-swift-container-relative-frame");
    expect(markup).toContain("cmux-custom-sidebar-swift-container-relative-frame-horizontal");
    expect(markup).toContain('data-swift-container-relative-frame-axis="horizontal"');
    expect(markup).toContain('data-swift-container-relative-frame-count="4"');
    expect(markup).toContain('data-swift-container-relative-frame-span="2"');
    expect(markup).toContain('data-swift-container-relative-frame-spacing="8"');
    expect(markup).toContain('data-swift-container-relative-frame-alignment="center"');
    expect(markup).toContain("width:calc(50% - 8px)");
    expect(markup).toContain("flex-basis:calc(50% - 8px)");
    expect(markup).toContain("cmux-custom-sidebar-swift-alignment-guide");
    expect(markup).toContain("cmux-custom-sidebar-swift-alignment-guide-leading");
    expect(markup).toContain('data-swift-alignment-guide="leading"');
    expect(markup).toContain('data-swift-alignment-guide-offset="12"');
    expect(markup).toContain("margin-left:12px");
    expect(markup).toContain("cmux-custom-sidebar-swift-visual-effect");
    expect(markup).toContain('data-swift-visual-effect="true"');
    expect(markup).toContain("cmux-custom-sidebar-swift-menu");
    expect(markup).toContain("<summary>Actions</summary>");
    expect(markup).toContain("Select");
    expect(markup).toContain("cmux-custom-sidebar-swift-control-group");
    expect(markup).toContain("cmux-custom-sidebar-swift-control-group-style-compactMenu");
    expect(markup).toContain('data-swift-control-group="true"');
    expect(markup).toContain('data-swift-control-group-style="compactMenu"');
    expect(markup).toContain(">Run<");
    expect(markup).toContain(">Stop<");
    expect(markup).toContain("Windows/Tauri Swift subset renderer");
  });

  test("renders Swift split views with independently scrollable panes", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [];
    const source = `
      HSplitView {
        VStack {
          Text("Left")
        }
        VSplitView {
          Text("Top")
          Text("Bottom")
        }
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\split.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("cmux-custom-sidebar-swift-split-horizontal");
    expect(markup).toContain("cmux-custom-sidebar-swift-split-vertical");
    expect(markup).toContain("cmux-custom-sidebar-swift-split-pane");
    expect(markup).toContain(">Left<");
    expect(markup).toContain(">Top<");
    expect(markup).toContain(">Bottom<");
    expect(markup).not.toContain("Unsupported Swift view");
  });

  test("renders authored Swift modifiers as styles and fill classes", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [];
    const source = `
      VStack(alignment: .leading, spacing: 8) {
        Text("Styled")
          .font(.headline)
          .bold()
          .foregroundColor(.green)
        Text("Muted")
          .foregroundStyle(.tertiary)
        Text("Action")
          .tint(.accent)
        Text("Tracked")
          .tracking(1.5)
        Text("Kerned")
          .kerning(2)
        Text("Raised")
          .baselineOffset(3)
        Text("Sized")
          .frame(width: 120, height: 32, minWidth: 80, idealHeight: 28, maxHeight: 44, alignment: .trailing)
        HStack(alignment: .top) {
          Text("Card")
        }
          .padding(10)
          .padding(.horizontal, 6)
          .padding([.top, .bottom], 4)
          .background(.teal)
          .cornerRadius(14)
          .frame(maxWidth: .infinity, alignment: .trailing)
        HStack {
          Text("Insets")
        }
          .padding(EdgeInsets(top: 1, leading: 2, bottom: 3, trailing: 4))
        HStack {
          Text("Safe")
        }
          .safeAreaPadding(.horizontal, 12)
        HStack {
          Text("Content")
        }
          .contentMargins(.bottom, 5, for: .scrollContent)
        HStack {
          Text("Safe Insets")
        }
          .safeAreaPadding(EdgeInsets(top: 2, leading: 3, bottom: 4, trailing: 5))
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\styled.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("Styled");
    expect(markup).toContain("font-size:14px");
    expect(markup).toContain("font-weight:760");
    expect(markup).toContain("color:#86efac");
    expect(markup).toContain("cmux-custom-sidebar-swift-foreground-tertiary");
    expect(markup).toContain("color:#6f8580");
    expect(markup).toContain("color:#2dd4bf");
    expect(markup).toContain("cmux-custom-sidebar-swift-tracking");
    expect(markup).toContain("letter-spacing:1.5px");
    expect(markup).toContain("cmux-custom-sidebar-swift-kerning");
    expect(markup).toContain("letter-spacing:2px");
    expect(markup).toContain("cmux-custom-sidebar-swift-baseline-offset");
    expect(markup).toContain("vertical-align:3px");
    expect(markup).toContain("width:120px");
    expect(markup).toContain("height:32px");
    expect(markup).toContain("min-width:80px");
    expect(markup).toContain("max-height:44px");
    expect(markup).toContain("cmux-custom-sidebar-swift-stack-alignment-leading");
    expect(markup).toContain("cmux-custom-sidebar-swift-stack-alignment-top");
    expect(markup).toContain("gap:8px");
    expect(markup).toContain("align-items:flex-start");
    expect(markup).toContain("cmux-custom-sidebar-swift-frame-trailing");
    expect(markup).toContain("text-align:right");
    expect(markup).toContain("justify-content:flex-end");
    expect(markup).toContain("cmux-custom-sidebar-swift-fill");
    expect(markup).toContain("padding:10px");
    expect(markup).toContain("padding-left:6px");
    expect(markup).toContain("padding-right:6px");
    expect(markup).toContain("padding-top:4px");
    expect(markup).toContain("padding-bottom:4px");
    expect(markup).toContain(">Insets<");
    expect(markup).toContain("padding-top:1px");
    expect(markup).toContain("padding-left:2px");
    expect(markup).toContain("padding-bottom:3px");
    expect(markup).toContain("padding-right:4px");
    expect(markup).toContain("cmux-custom-sidebar-swift-safe-area-padding");
    expect(markup).toContain("cmux-custom-sidebar-swift-safe-area-padding-leading");
    expect(markup).toContain("cmux-custom-sidebar-swift-safe-area-padding-trailing");
    expect(markup).toContain('data-swift-safe-area-padding-edges="leading,trailing"');
    expect(markup).toContain('data-swift-safe-area-padding-length="12"');
    expect(markup).toContain("padding-left:12px");
    expect(markup).toContain("padding-right:12px");
    expect(markup).toContain("cmux-custom-sidebar-swift-content-margins");
    expect(markup).toContain("cmux-custom-sidebar-swift-content-margins-bottom");
    expect(markup).toContain("cmux-custom-sidebar-swift-content-margins-scrollContent");
    expect(markup).toContain('data-swift-content-margins-edges="bottom"');
    expect(markup).toContain('data-swift-content-margins-length="5"');
    expect(markup).toContain('data-swift-content-margins-placement="scrollContent"');
    expect(markup).toContain("padding-bottom:5px");
    expect(markup).toContain('data-swift-safe-area-padding-insets="top:2,leading:3,bottom:4,trailing:5"');
    expect(markup).toContain("padding-top:2px");
    expect(markup).toContain("padding-left:3px");
    expect(markup).toContain("padding-bottom:4px");
    expect(markup).toContain("padding-right:5px");
    expect(markup).toContain("border-radius:14px");
    expect(markup).toContain("Windows/Tauri Swift subset renderer");
  });

  test("renders SwiftUI child-bearing modifiers", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [workspace({ workspace_id: "workspace-a", custom_title: "Ops" })];
    const source = `
      @State var query = "ops"

      Text("Base")
        .background {
          RoundedRectangle(cornerRadius: 8).fill(.blue)
        }
        .overlay(alignment: .topTrailing) {
          Text("Badge")
        }
        .safeAreaInset(edge: .bottom) {
          Text("Inset")
        }
        .contextMenu {
          Button("Open") { cmux("workspace.select", workspace_id: selectedId) }
        }
        .refreshable {
          cmux("sidebar.reload", name: sourceName)
        }
        .swipeActions(edge: .leading, allowsFullSwipe: false) {
          Button("Pin") { cmux("workspace.select", workspace_id: selectedId) }
          Button("Remove", role: .destructive) { cmux("sidebar.reload", name: sourceName) }
        }
        .searchable(text: $query, placement: .sidebar, prompt: "Filter")
        .accessibilityRepresentation {
          Label("Voice row", systemImage: "speaker.wave.2")
        }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\child-modifiers.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("cmux-custom-sidebar-swift-modified");
    expect(markup).toContain("cmux-custom-sidebar-swift-modifier-background");
    expect(markup).toContain("cmux-custom-sidebar-swift-modifier-overlay-topTrailing");
    expect(markup).toContain("cmux-custom-sidebar-swift-safe-area-inset");
    expect(markup).toContain("cmux-custom-sidebar-swift-context-menu");
    expect(markup).toContain("cmux-custom-sidebar-swift-refreshable");
    expect(markup).toContain('data-swift-refreshable="true"');
    expect(markup).toContain("cmux-custom-sidebar-swift-swipe-actions-leading");
    expect(markup).toContain('data-swift-swipe-actions="leading"');
    expect(markup).toContain('data-swift-swipe-allows-full-swipe="false"');
    expect(markup).toContain("cmux-custom-sidebar-swift-searchable");
    expect(markup).toContain('data-swift-searchable="true"');
    expect(markup).toContain('data-swift-search-placement="sidebar"');
    expect(markup).toContain('data-swift-state-binding="query"');
    expect(markup).toContain('type="search"');
    expect(markup).toContain('value="ops"');
    expect(markup).toContain('placeholder="Filter"');
    expect(markup).toContain('data-swift-search-prompt="Filter"');
    expect(markup).toContain("cmux-custom-sidebar-swift-accessibility-representation");
    expect(markup).toContain('data-swift-accessibility-representation="true"');
    expect(markup).toContain('data-swift-accessibility-representation-count="1"');
    expect(markup).toContain('data-swift-accessibility-representation-label="Voice row"');
    expect(markup).toContain(
      "cmux-custom-sidebar-swift-accessibility-representation-content",
    );
    expect(markup).toContain('data-swift-accessibility-representation-content="true"');
    expect(markup).toContain('aria-hidden="true"');
    expect(markup).toContain(">Base<");
    expect(markup).toContain(">Badge<");
    expect(markup).toContain(">Inset<");
    expect(markup).toContain(">Open<");
    expect(markup).toContain(">Refresh<");
    expect(markup).toContain(">Pin<");
    expect(markup).toContain(">Remove<");
    expect(markup).toContain(">Voice row<");
    expect(markup).not.toContain("Unsupported Swift view");
  });

  test("renders SwiftUI local presentation modifiers", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [];
    const source = `
      @State var showSheet = true
      @State var showPopover = true
      @State var showAlert = true
      @State var selectedPanel = "Details"
      @State var selectedWarning = "Build failed"
      @State var selectedAction = "Deploy"

      Text("Base")
        .sheet(isPresented: $showSheet) {
          Text("Sheet body")
            .presentationDetents([.medium, .large])
            .presentationDragIndicator(.visible)
            .presentationBackground(.regularMaterial)
            .presentationCornerRadius(22)
        }
        .sheet(item: $selectedPanel) { panel in
          Text(panel)
          Button("Done") { dismiss() }
        }
        .popover(isPresented: $showPopover) {
          Text("Popover body")
        }
        .alert("Heads up", isPresented: $showAlert) {
          Text("Alert body")
          Button("Delete", role: .destructive) { dismiss() }
          Button("Cancel", role: .cancel) { dismiss() }
        }
        .alert("Warning", item: $selectedWarning) { warning in
          Text(warning)
          Button("Acknowledge", role: .cancel) { dismiss() }
        }
        .confirmationDialog("Action menu", item: $selectedAction) { action in
          Text(action)
          Button("Run") { dismiss() }
        }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\presentations.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("cmux-custom-sidebar-swift-presentation-sheet");
    expect(markup).toContain("cmux-custom-sidebar-swift-presentation-detents-mediumlarge");
    expect(markup).toContain("cmux-custom-sidebar-swift-presentation-drag-indicator-visible");
    expect(markup).toContain("cmux-custom-sidebar-swift-presentation-background-regularMaterial");
    expect(markup).toContain("cmux-custom-sidebar-swift-presentation-corner-radius");
    expect(markup).toContain("cmux-custom-sidebar-swift-presentation-popover");
    expect(markup).toContain("cmux-custom-sidebar-swift-presentation-alert");
    expect(markup).toContain("cmux-custom-sidebar-swift-presentation-confirmationDialog");
    expect(markup).toContain("role=\"dialog\"");
    expect(markup).toContain("role=\"alertdialog\"");
    expect(markup).toContain(">Sheet body<");
    expect(markup).toContain('data-swift-presentation-detents="medium,large"');
    expect(markup).toContain('data-swift-presentation-drag-indicator="visible"');
    expect(markup).toContain('data-swift-presentation-background="regularMaterial"');
    expect(markup).toContain('data-swift-presentation-corner-radius="22"');
    expect(markup).toContain("border-radius:22px");
    expect(markup).toContain(">Details<");
    expect(markup).toContain(">Done<");
    expect(markup).toContain('data-swift-presentation-binding="item"');
    expect(markup).toContain('data-swift-presentation-item="Details"');
    expect(markup).toContain('data-swift-local-action="dismissPresentation"');
    expect(markup).toContain(">Popover body<");
    expect(markup).toContain(">Heads up<");
    expect(markup).toContain(">Alert body<");
    expect(markup).toContain(">Delete<");
    expect(markup).toContain(">Cancel<");
    expect(markup).toContain(">Warning<");
    expect(markup).toContain(">Build failed<");
    expect(markup).toContain('data-swift-presentation-item="Build failed"');
    expect(markup).toContain(">Acknowledge<");
    expect(markup).toContain(">Action menu<");
    expect(markup).toContain(">Deploy<");
    expect(markup).toContain('data-swift-presentation-item="Deploy"');
    expect(markup).toContain(">Run<");
    expect(markup).toContain("cmux-custom-sidebar-swift-presentation-actions");
    expect(markup).toContain("cmux-custom-sidebar-swift-button-destructive");
    expect(markup).toContain("cmux-custom-sidebar-swift-button-cancel");
    expect(markup).toContain(">Close<");
    expect(markup).not.toContain("Unsupported Swift view");
  });

  test("renders SwiftUI static navigation, toolbar, and shortcut modifiers", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [workspace({ workspace_id: "workspace-a", custom_title: "Ops" })];
    const source = `
      VStack {
        Text("Body")
      }
        .navigationTitle("Board")
        .navigationSubtitle(selectedTitle)
        .navigationBarTitleDisplayMode(.inline)
        .toolbarBackground(.blue, for: .navigationBar)
        .toolbarColorScheme(.dark, for: .navigationBar)
        .toolbar {
          ToolbarItem(placement: .primaryAction) {
            Button("Refresh") { cmux("sidebar.reload", name: sourceName) }
          }
        }
        .keyboardShortcut("b", modifiers: [.command, .shift])
        .contentShape(Capsule())
        .draggable(selectedId)
        .dropDestination(for: String.self) { items, location in
          cmux("sidebar.reload", name: sourceName)
        }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\navigation.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("cmux-custom-sidebar-swift-navigation-inline");
    expect(markup).toContain("cmux-custom-sidebar-swift-navigation-title");
    expect(markup).toContain(">Board<");
    expect(markup).toContain("cmux-custom-sidebar-swift-navigation-subtitle");
    expect(markup).toContain(">Ops<");
    expect(markup).toContain("cmux-custom-sidebar-swift-toolbar");
    expect(markup).toContain("cmux-custom-sidebar-swift-toolbar-background-blue");
    expect(markup).toContain("cmux-custom-sidebar-swift-toolbar-background-for-navigationBar");
    expect(markup).toContain("cmux-custom-sidebar-swift-toolbar-color-scheme-dark");
    expect(markup).toContain("cmux-custom-sidebar-swift-toolbar-color-scheme-for-navigationBar");
    expect(markup).toContain('data-swift-toolbar-background="blue"');
    expect(markup).toContain('data-swift-toolbar-background-for="navigationBar"');
    expect(markup).toContain('data-swift-toolbar-color-scheme="dark"');
    expect(markup).toContain('data-swift-toolbar-color-scheme-for="navigationBar"');
    expect(markup).toContain("background:rgba(59, 130, 246, 0.14)");
    expect(markup).toContain("color-scheme:dark");
    expect(markup).toContain("cmux-custom-sidebar-swift-toolbar-item-primaryAction");
    expect(markup).toContain('data-swift-toolbar-placement="primaryAction"');
    expect(markup).toContain(">Refresh<");
    expect(markup).toContain("aria-keyshortcuts=\"Meta+Shift+b\"");
    expect(markup).toContain("cmux-custom-sidebar-swift-keyboard-shortcut-command");
    expect(markup).toContain("cmux-custom-sidebar-swift-keyboard-shortcut-shift");
    expect(markup).toContain("cmux-custom-sidebar-swift-content-shape-capsule");
    expect(markup).toContain('data-swift-content-shape="capsule"');
    expect(markup).toContain("cmux-custom-sidebar-swift-draggable");
    expect(markup).toContain("cmux-custom-sidebar-swift-drop-destination");
    expect(markup).toContain("cmux-custom-sidebar-swift-drop-destination-String");
    expect(markup).toContain('data-swift-drop-destination="String"');
    expect(markup).toContain("draggable=\"true\"");
    expect(markup).toContain('data-swift-draggable="workspace-a"');
    expect(markup).not.toContain("Unsupported Swift view");
  });

  test("renders SwiftUI NavigationStack and NavigationLink rows", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [workspace({ workspace_id: "workspace-a", custom_title: "Ops" })];
    const source = `
      NavigationStack {
        NavigationLink("Workspace detail") {
          VStack {
            Text(selectedTitle)
            Button("Focus") { cmux("workspace.select", workspace_id: selectedId) }
          }
        }
        NavigationLink(destination: Text("Settings")) {
          Label("Settings", systemImage: "gear")
        }
        NavigationLink(value: selectedId) {
          Label("Route detail", systemImage: "arrow.right")
        }
      }
      .navigationDestination(for: String.self) { route in
        VStack {
          Text(route)
          Text("Route destination")
        }
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\navigation-stack.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("cmux-custom-sidebar-swift-navigation-stack");
    expect(markup).toContain("cmux-custom-sidebar-swift-navigation-link");
    expect(markup).toContain(">Workspace detail<");
    expect(markup).toContain(">Settings<");
    expect(markup).toContain(">Route detail<");
    expect(markup).toContain('data-navigation-value="workspace-a"');
    expect(markup).not.toContain(">Focus<");
    expect(markup).not.toContain(">Route destination<");
    expect(markup).not.toContain("cmux-custom-sidebar-swift-navigation-link-disabled");
    expect(markup).not.toContain("Unsupported Swift view");
  });

  test("renders richer SwiftUI leaf modifiers as styles and attributes", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [];
    const source = `
      VStack {
        Text("Long copy")
          .italic()
          .bold(false)
          .fontWeight(.semibold)
          .fontDesign(.rounded)
          .fontWidth(.condensed)
          .dynamicTypeSize(.accessibility2)
          .monospaced()
          .monospacedDigit()
          .lineLimit(2, reservesSpace: true)
          .multilineTextAlignment(.center)
          .opacity(0.5)
          .hidden()
          .fixedSize()
          .allowsHitTesting(false)
          .help("Explains the copy")
          .accessibilityLabel("Readable copy")
          .accessibilityValue("42 unread")
          .accessibilityHint("Opens details")
          .accessibilityAddTraits([.isButton, .isHeader])
          .accessibilitySortPriority(3)
          .accessibilityAction(named: Text("Refresh")) {
            cmux("sidebar.reload", name: sourceName)
          }
          .accessibilityActivationPoint(CGPoint(x: 12, y: 24))
          .animation(.easeInOut, value: selectedId)
          .transition(.opacity)
          .contentTransition(.opacity)
          .symbolEffect(.pulse, isActive: true, value: selectedId)
          .preferredColorScheme(.dark)
          .environment(\.colorScheme, .light)
          .environment(\.layoutDirection, .rightToLeft)
          .id("copy-row")
          .onGeometryChange(for: CGSize.self) { proxy in proxy.size }
        Button("Nope") { cmux("workspace.select", workspace_id: selectedId) }
          .disabled(true)
          .help("Disabled action")
        Text("Secret")
          .redacted(reason: .placeholder)
        Text("Private token")
          .privacySensitive()
        Text("Visible token")
          .redacted(reason: [.placeholder, .privacy])
          .unredacted()
        Text("Hidden")
          .accessibilityHidden(true)
        HStack {
          Text("Primary")
          Text("Secondary")
        }
          .accessibilityElement(children: .combine)
        Text("Calm symbol")
          .symbolEffect(.bounce, isActive: true)
          .symbolEffectsRemoved()
        Text("Plain style")
          .bold(false)
          .italic(false)
          .hoverEffect(.lift)
          .defaultHoverEffect(.highlight)
        Text("No hover")
          .hoverEffect(.highlight, isEnabled: false)
        Text("Inbox")
          .badge(3)
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\rich.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("font-style:italic");
    expect(markup).toContain("font-weight:650");
    expect(markup).toContain(">Plain style<");
    expect(markup).toContain("cmux-custom-sidebar-swift-font-design-rounded");
    expect(markup).toContain("cmux-custom-sidebar-swift-font-width-condensed");
    expect(markup).toContain("font-stretch:87.5%");
    expect(markup).toContain("cmux-custom-sidebar-swift-dynamic-type-accessibility2");
    expect(markup).toContain('data-swift-dynamic-type-size="accessibility2"');
    expect(markup).toContain("font-size:1.78rem");
    expect(markup).toContain("cmux-custom-sidebar-swift-monospace");
    expect(markup).toContain("cmux-custom-sidebar-swift-monospaced-digit");
    expect(markup).toContain("font-variant-numeric:tabular-nums");
    expect(markup).toContain("-webkit-line-clamp:2");
    expect(markup).toContain("cmux-custom-sidebar-swift-line-limit-reserves-space");
    expect(markup).toContain("min-height:calc(2 * 1.35em)");
    expect(markup).toContain("text-align:center");
    expect(markup).toContain("opacity:0.5");
    expect(markup).toContain("cmux-custom-sidebar-swift-hidden");
    expect(markup).toContain('data-swift-hidden="true"');
    expect(markup).toContain("visibility:hidden");
    expect(markup).toContain("cmux-custom-sidebar-swift-fixed-size");
    expect(markup).toContain("cmux-custom-sidebar-swift-allows-hit-testing-off");
    expect(markup).toContain('data-swift-allows-hit-testing="false"');
    expect(markup).toContain("pointer-events:none");
    expect(markup).toContain("cmux-custom-sidebar-swift-identified");
    expect(markup).toContain('data-swift-id="copy-row"');
    expect(markup).toContain("aria-label=\"Readable copy\"");
    expect(markup).toContain("aria-valuetext=\"42 unread\"");
    expect(markup).toContain('data-swift-accessibility-hint="Opens details"');
    expect(markup).toContain('data-swift-accessibility-traits="isButton,isHeader"');
    expect(markup).toContain('data-swift-accessibility-sort-priority="3"');
    expect(markup).toContain("cmux-custom-sidebar-swift-accessibility-trait-button");
    expect(markup).toContain("cmux-custom-sidebar-swift-accessibility-action");
    expect(markup).toContain("cmux-custom-sidebar-swift-accessibility-action-Refresh");
    expect(markup).toContain('data-swift-accessibility-action="Refresh"');
    expect(markup).toContain('data-swift-accessibility-action-enabled="true"');
    expect(markup).toContain('tabindex="0"');
    expect(markup).toContain("cmux-custom-sidebar-swift-accessibility-activation-point");
    expect(markup).toContain('data-swift-accessibility-activation-point-x="12"');
    expect(markup).toContain('data-swift-accessibility-activation-point-y="24"');
    expect(markup).toContain('data-swift-accessibility-element-children="combine"');
    expect(markup).toContain("cmux-custom-sidebar-swift-accessibility-element-combine");
    expect(markup).toContain("cmux-custom-sidebar-swift-animation-easeInOut");
    expect(markup).toContain('data-swift-animation="easeInOut"');
    expect(markup).toContain('data-swift-animation-value="selectedId"');
    expect(markup).toContain("cmux-custom-sidebar-swift-transition-opacity");
    expect(markup).toContain('data-swift-transition="opacity"');
    expect(markup).toContain("cmux-custom-sidebar-swift-content-transition-opacity");
    expect(markup).toContain('data-swift-content-transition="opacity"');
    expect(markup).toContain("cmux-custom-sidebar-swift-on-geometry-change");
    expect(markup).toContain("cmux-custom-sidebar-swift-on-geometry-change-CGSize");
    expect(markup).toContain('data-swift-on-geometry-change="true"');
    expect(markup).toContain('data-swift-on-geometry-change-type="CGSize"');
    expect(markup).toContain("cmux-custom-sidebar-swift-symbol-effect-pulse");
    expect(markup).toContain('data-swift-symbol-effect="pulse"');
    expect(markup).toContain('data-swift-symbol-effect-active="true"');
    expect(markup).toContain('data-swift-symbol-effect-value="selectedId"');
    expect(markup).toContain("cmux-custom-sidebar-swift-preferred-color-scheme-dark");
    expect(markup).toContain('data-swift-preferred-color-scheme="dark"');
    expect(markup).toContain("cmux-custom-sidebar-swift-environment-color-scheme-light");
    expect(markup).toContain('data-swift-environment-color-scheme="light"');
    expect(markup).toContain("cmux-custom-sidebar-swift-environment-layout-direction-rightToLeft");
    expect(markup).toContain('data-swift-environment-layout-direction="rightToLeft"');
    expect(markup).toContain("color-scheme:light");
    expect(markup).toContain("direction:rtl");
    expect(markup).toContain("cmux-custom-sidebar-swift-symbol-effects-removed");
    expect(markup).toContain('data-swift-symbol-effects-removed="true"');
    expect(markup).not.toContain("cmux-custom-sidebar-swift-symbol-effect-bounce");
    expect(markup).not.toContain('data-swift-symbol-effect="bounce"');
    expect(markup).toContain("cmux-custom-sidebar-swift-hover-effect");
    expect(markup).toContain("cmux-custom-sidebar-swift-hover-effect-lift");
    expect(markup).toContain('data-swift-hover-effect="lift"');
    expect(markup).toContain('data-swift-hover-effect-enabled="true"');
    expect(markup).toContain("cmux-custom-sidebar-swift-default-hover-effect");
    expect(markup).toContain("cmux-custom-sidebar-swift-default-hover-effect-highlight");
    expect(markup).toContain('data-swift-default-hover-effect="highlight"');
    expect(markup).toContain("cmux-custom-sidebar-swift-hover-effect-disabled");
    expect(markup).toContain('data-swift-hover-effect="highlight"');
    expect(markup).toContain('data-swift-hover-effect-enabled="false"');
    expect(markup).toContain(">Inbox<");
    expect(markup).toContain("cmux-custom-sidebar-swift-badge");
    expect(markup).toContain('data-swift-badge="3"');
    expect(markup).toContain("title=\"Explains the copy\"");
    expect(markup).toContain("title=\"Disabled action\"");
    expect(markup).toContain("disabled=\"\"");
    expect(markup).toContain("cmux-custom-sidebar-swift-redacted");
    expect(markup).toContain("cmux-custom-sidebar-swift-redacted-placeholder");
    expect(markup).toContain('data-swift-redacted="true"');
    expect(markup).toContain('data-swift-redaction-reason="placeholder"');
    expect(markup).toContain("cmux-custom-sidebar-swift-privacy-sensitive");
    expect(markup).toContain('data-swift-privacy-sensitive="true"');
    expect(markup).toContain('data-swift-redaction-reason="privacy"');
    expect(markup).toContain("cmux-custom-sidebar-swift-unredacted");
    expect(markup).toContain('data-swift-unredacted="true"');
    expect(markup).toContain('data-swift-redaction-reason="placeholder,privacy"');
    expect(markup).toContain("aria-hidden=\"true\"");
  });

  test("renders false SwiftUI bold and italic modifiers as inactive", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [];
    const source = `
      Text("Plain style")
        .bold(false)
        .italic(false)
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\plain-style.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain(">Plain style<");
    expect(markup).not.toContain("font-style:italic");
    expect(markup).not.toContain("font-weight:760");
    expect(markup).not.toContain("Unsupported Swift view");
  });

  test("renders text, list, scroll, and symbol presentation modifiers", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [];
    const source = `
      List {
        Text("stale branch")
          .truncationMode(.tail)
          .textCase(.uppercase)
          .underline(true, pattern: .dash, color: .mint)
          .strikethrough(true, pattern: .dot, color: .red)
        Image(systemName: "bolt.fill")
          .resizable(capInsets: EdgeInsets(top: 2, leading: 4, bottom: 6, trailing: 8), resizingMode: .tile)
          .scaledToFit()
          .renderingMode(.template)
          .interpolation(.none)
          .antialiased(false)
          .flipsForRightToLeftLayoutDirection(true)
          .imageScale(.large)
          .symbolRenderingMode(.hierarchical)
          .symbolVariant(.fill)
        Image("product.logo")
          .resizable()
        Image(decorative: "badge.icon")
          .accessibilityLabel("Decorative badge")
        Image("secret.logo")
        Image("missing.logo")
        AsyncImage(url: URL(string: "https://example.com/avatar.png"))
          .resizable()
          .scaledToFill()
        AsyncImage(
          url: URL(string: "https://example.com/hero.png"),
          content: { image in
            image
              .resizable()
              .scaledToFill()
          },
          placeholder: {
            ProgressView(value: 0.25)
          }
        )
        AsyncImage(url: URL(string: "https://example.com/trailing.png")) { image in
          image
            .resizable()
        } placeholder: {
          Text("Loading trailing")
        }
          .frame(width: 32, height: 24)
        AsyncImage(url: "file:///etc/passwd")
        Text("SHOUT")
          .truncationMode(.head)
          .textCase(.lowercase)
          .multilineTextAlignment(.trailing)
        Text("middle cut")
          .truncationMode(.middle)
      }
        .listStyle(.sidebar)
        .scrollContentBackground(.hidden)
        .scrollIndicators(.hidden, axes: .vertical)
        .scrollClipDisabled()
        .scrollTargetBehavior(.paging)
        .scrollTargetLayout()
        .scrollBounceBehavior(.basedOnSize, axes: .vertical)
        .scrollDisabled(true)
        .scrollPosition(id: "workspace-a", anchor: .center)
        .defaultScrollAnchor(.bottom)
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\presentation.swift"
        sourceOverride={source}
        assetOverride={{
          "product.logo": "https://example.com/product.png",
          "badge.icon": "cmux-sidebar-asset://ops/badge.svg",
          "secret.logo": "file:///etc/passwd",
        }}
      />,
    );

    expect(markup).toContain("cmux-custom-sidebar-swift-truncate-tail");
    expect(markup).toContain("cmux-custom-sidebar-swift-truncate-head");
    expect(markup).toContain("cmux-custom-sidebar-swift-truncate-middle");
    expect(markup).toContain("text-transform:uppercase");
    expect(markup).toContain("text-transform:lowercase");
    expect(markup).toContain("text-align:right");
    expect(markup).toContain("cmux-custom-sidebar-swift-underline");
    expect(markup).toContain("cmux-custom-sidebar-swift-underline-dash");
    expect(markup).toContain("cmux-custom-sidebar-swift-strikethrough");
    expect(markup).toContain("cmux-custom-sidebar-swift-strikethrough-dot");
    expect(markup).toContain("text-decoration-style:dotted");
    expect(markup).toContain("text-decoration-color:#fda4af");
    expect(markup).toContain("cmux-custom-sidebar-swift-list-style-sidebar");
    expect(markup).toContain("cmux-custom-sidebar-swift-scroll-content-background-hidden");
    expect(markup).toContain("cmux-custom-sidebar-swift-scroll-indicators-hidden");
    expect(markup).toContain("cmux-custom-sidebar-swift-scroll-indicators-axis-vertical");
    expect(markup).toContain("cmux-custom-sidebar-swift-scroll-clip-disabled");
    expect(markup).toContain("cmux-custom-sidebar-swift-scroll-target-behavior-paging");
    expect(markup).toContain("cmux-custom-sidebar-swift-scroll-target-layout");
    expect(markup).toContain("cmux-custom-sidebar-swift-scroll-bounce-basedOnSize");
    expect(markup).toContain("cmux-custom-sidebar-swift-scroll-bounce-axis-vertical");
    expect(markup).toContain("cmux-custom-sidebar-swift-scroll-disabled");
    expect(markup).toContain("cmux-custom-sidebar-swift-scroll-position");
    expect(markup).toContain("cmux-custom-sidebar-swift-scroll-position-anchor-center");
    expect(markup).toContain("cmux-custom-sidebar-swift-default-scroll-anchor-bottom");
    expect(markup).toContain('data-swift-scroll-target-behavior="paging"');
    expect(markup).toContain('data-swift-scroll-target-layout="true"');
    expect(markup).toContain('data-swift-scroll-bounce-behavior="basedOnSize"');
    expect(markup).toContain('data-swift-scroll-bounce-axes="vertical"');
    expect(markup).toContain('data-swift-scroll-disabled="true"');
    expect(markup).toContain('data-swift-scroll-position-id="workspace-a"');
    expect(markup).toContain('data-swift-scroll-position-anchor="center"');
    expect(markup).toContain('data-swift-default-scroll-anchor="bottom"');
    expect(markup).toContain("cmux-custom-sidebar-swift-image-resizable");
    expect(markup).toContain("cmux-custom-sidebar-swift-image-resizing-tile");
    expect(markup).toContain("cmux-custom-sidebar-swift-aspect-fit");
    expect(markup).toContain("cmux-custom-sidebar-swift-aspect-fill");
    expect(markup).toContain("cmux-custom-sidebar-swift-image-cap-insets");
    expect(markup).toContain("--cmux-custom-sidebar-swift-cap-inset-top:2px");
    expect(markup).toContain("--cmux-custom-sidebar-swift-cap-inset-leading:4px");
    expect(markup).toContain("--cmux-custom-sidebar-swift-cap-inset-bottom:6px");
    expect(markup).toContain("--cmux-custom-sidebar-swift-cap-inset-trailing:8px");
    expect(markup).toContain("cmux-custom-sidebar-swift-image-rendering-template");
    expect(markup).toContain("cmux-custom-sidebar-swift-image-interpolation-none");
    expect(markup).toContain("cmux-custom-sidebar-swift-image-antialiased-off");
    expect(markup).toContain("cmux-custom-sidebar-swift-flips-for-rtl");
    expect(markup).toContain('data-swift-flips-for-rtl="true"');
    expect(markup).toContain("scale:-1 1");
    expect(markup).toContain("cmux-custom-sidebar-swift-image-scale-large");
    expect(markup).toContain("cmux-custom-sidebar-swift-symbol-rendering-hierarchical");
    expect(markup).toContain("cmux-custom-sidebar-swift-symbol-variant-fill");
    expect(markup).toContain('data-swift-system-image="bolt.fill"');
    expect(markup).toContain('data-swift-system-image-glyph="bolt"');
    expect(markup).toContain("cmux-custom-sidebar-swift-asset-image");
    expect(markup).toContain('src="https://example.com/product.png"');
    expect(markup).toContain('src="cmux-sidebar-asset://ops/badge.svg"');
    expect(markup).toContain('alt=""');
    expect(markup).toContain('aria-hidden="true"');
    expect(markup).not.toContain("Decorative badge");
    expect(markup).toContain("Missing Image asset: secret.logo");
    expect(markup).toContain("Missing Image asset: missing.logo");
    expect(markup).toContain("cmux-custom-sidebar-swift-async-image");
    expect(markup).toContain('src="https://example.com/avatar.png"');
    expect(markup).toContain('data-swift-async-image-phase="success"');
    expect(markup).toContain('data-swift-async-image-url="https://example.com/avatar.png"');
    expect(markup).toContain("cmux-custom-sidebar-swift-async-image-content");
    expect(markup).toContain('data-swift-async-image-url="https://example.com/hero.png"');
    expect(markup).toContain('data-swift-async-image-content-count="1"');
    expect(markup).toContain('data-swift-async-image-placeholder-count="1"');
    expect(markup).toContain('src="https://example.com/hero.png"');
    expect(markup).toContain('data-swift-async-image-url="https://example.com/trailing.png"');
    expect(markup).toContain('src="https://example.com/trailing.png"');
    expect(markup).toContain("width:32px");
    expect(markup).toContain("height:24px");
    expect(markup).toContain('loading="lazy"');
    expect(markup).toContain('referrerPolicy="no-referrer"');
    expect(markup).toContain("Unsupported AsyncImage URL");
    expect(markup).toContain('data-swift-async-image-phase="failure"');
    expect(markup).not.toContain("file:///etc/passwd");
  });

  test("renders built-in control style modifiers", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [];
    const source = `
      VStack {
        Label("Repo", systemImage: "folder")
          .labelStyle(.iconOnly)
        Button("Run") { cmux("workspace.select", workspace_id: selectedId) }
          .controlSize(.small)
          .buttonStyle(.borderedProminent)
          .buttonBorderShape(.capsule)
        Toggle("Pinned", isOn: .constant(true))
          .toggleStyle(.button)
        TextField("Title", text: .constant(selectedTitle))
          .textFieldStyle(.roundedBorder)
          .labelsHidden()
        Picker("Lane", selection: .constant(selectedId)) {
          Text(selectedTitle)
        }
          .pickerStyle(.segmented)
        Menu("Actions") {
          Button("Reload") { cmux("sidebar.reload", name: sourceName) }
        }
          .menuStyle(.button)
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\control-styles.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("cmux-custom-sidebar-swift-label-style-iconOnly");
    expect(markup).toContain("cmux-custom-sidebar-swift-control-size-small");
    expect(markup).toContain("cmux-custom-sidebar-swift-button-style-borderedProminent");
    expect(markup).toContain("cmux-custom-sidebar-swift-button-border-shape-capsule");
    expect(markup).toContain("cmux-custom-sidebar-swift-toggle-style-button");
    expect(markup).toContain("cmux-custom-sidebar-swift-text-field-style-roundedBorder");
    expect(markup).toContain("cmux-custom-sidebar-swift-labels-hidden");
    expect(markup).toContain('data-swift-labels-hidden="true"');
    expect(markup).toContain("cmux-custom-sidebar-swift-picker-style-segmented");
    expect(markup).toContain("cmux-custom-sidebar-swift-menu-style-button");
    expect(markup).not.toContain("Unsupported Swift view");
  });

  test("renders layout and decoration modifiers as safe styles", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [];
    const source = `
      VStack {
        Text("Badge")
          .layoutPriority(2)
          .offset(x: 4, y: -2)
          .position(x: 12, y: 24)
          .zIndex(3)
          .aspectRatio(1.5, contentMode: .fit)
          .clipped()
          .compositingGroup()
          .clipShape(Capsule(), style: FillStyle(eoFill: true, antialiased: false))
          .shadow(color: .black, radius: 6, x: 1, y: 2)
          .border(.gray, width: 2)
          .blur(radius: 1)
          .brightness(0.1)
          .contrast(1.2)
          .saturation(0.8)
          .grayscale(0.25)
          .hueRotation(Angle.radians(1.5707963267948966))
          .blendMode(.screen)
          .rotationEffect(Angle(degrees: 15))
          .scaleEffect(1.1)
          .rotation3DEffect(Angle.degrees(30), axis: (x: 0, y: 1, z: 0), anchor: UnitPoint(x: 0, y: 0), perspective: 0.7)
        Circle()
          .fill(.green)
          .stroke(.blue, width: 2)
        RoundedRectangle(cornerRadius: 10)
          .strokeBorder(.orange, lineWidth: 4)
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\decor.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("cmux-custom-sidebar-swift-aspect-fit");
    expect(markup).toContain("cmux-custom-sidebar-swift-clip-capsule");
    expect(markup).toContain("cmux-custom-sidebar-swift-clip-style-eoFill");
    expect(markup).toContain("cmux-custom-sidebar-swift-clip-antialiased-off");
    expect(markup).toContain('data-swift-clip-style="eoFill"');
    expect(markup).toContain('data-swift-clip-antialiased="false"');
    expect(markup).toContain("cmux-custom-sidebar-swift-positioned");
    expect(markup).toContain("flex-grow:2");
    expect(markup).toContain("left:12px");
    expect(markup).toContain("top:24px");
    expect(markup).toContain("z-index:3");
    expect(markup).toContain("aspect-ratio:1.5");
    expect(markup).toContain("overflow:hidden");
    expect(markup).toContain("cmux-custom-sidebar-swift-compositing-group");
    expect(markup).toContain("isolation:isolate");
    expect(markup).toContain("box-shadow:1px 2px 6px rgba(0, 0, 0, 0.42)");
    expect(markup).toContain("border:2px solid #94a3b8");
    expect(markup).toContain("filter:blur(1px) brightness(1.1) contrast(1.2) saturate(0.8) grayscale(0.25) hue-rotate(90deg)");
    expect(markup).toContain("mix-blend-mode:screen");
    expect(markup).toContain("cmux-custom-sidebar-swift-blend-screen");
    expect(markup).toContain("cmux-custom-sidebar-swift-rotation3d");
    expect(markup).toContain("cmux-custom-sidebar-swift-rotation3d-anchor-topLeading");
    expect(markup).toContain("transform-origin:left top");
    expect(markup).toContain("transform:translate(4px, -2px) rotate(15deg) scale(1.1) perspective(700px) rotate3d(0, 1, 0, 30deg)");
    expect(markup).toContain("color:#86efac");
    expect(markup).toContain("border:2px solid #93c5fd");
    expect(markup).toContain("cmux-custom-sidebar-swift-shape-stroke-border");
    expect(markup).toContain('data-swift-shape-stroke="strokeBorder"');
    expect(markup).toContain('data-swift-shape-stroke-color="orange"');
    expect(markup).toContain('data-swift-shape-stroke-width="4"');
    expect(markup).toContain("border:4px solid #fdba74");
  });

  test("renders Swift gradient styles as safe CSS gradients", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [];
    const source = `
      VStack {
        Text("Linear")
          .background(LinearGradient(colors: [.red, .blue], startPoint: .topLeading, endPoint: .bottomTrailing))
        Text("Token")
          .foregroundStyle(Color.green.gradient)
        Text("Glass")
          .background(.regularMaterial)
        Text("Bar")
          .background(.bar)
        Rectangle()
          .fill(RadialGradient(colors: [.teal, .black], center: .center, startRadius: 0, endRadius: 24))
        Circle()
          .fill(AngularGradient(colors: [.yellow, .orange, .pink], center: .center))
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\gradients.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("linear-gradient(to bottom right, #fda4af, #93c5fd)");
    expect(markup).toContain("linear-gradient(135deg, #86efac, #86efac33)");
    expect(markup).toContain("cmux-custom-sidebar-swift-material-regularMaterial");
    expect(markup).toContain("cmux-custom-sidebar-swift-material-bar");
    expect(markup).toContain("linear-gradient(135deg, rgba(226, 232, 240, 0.13), rgba(15, 23, 42, 0.42))");
    expect(markup).toContain("linear-gradient(180deg, rgba(15, 23, 42, 0.78), rgba(2, 6, 23, 0.66))");
    expect(markup).toContain("backdrop-filter:blur(14px) saturate(1.2)");
    expect(markup).toContain("radial-gradient(circle, #67e8f9, #020617)");
    expect(markup).toContain("conic-gradient(#fde68a, #fdba74, #f9a8d4)");
    expect(markup).toContain("cmux-custom-sidebar-swift-gradient-foreground");
    expect(markup).not.toContain("Unsupported Swift view");
  });

  test("renders Swift array helper loops and enumerated pairs", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [
      workspace({ workspace_id: "workspace-a", custom_title: "Ops lane" }),
      workspace({ workspace_id: "workspace-b", custom_title: "API lane" }),
      workspace({ workspace_id: "workspace-c", custom_title: "Docs lane" }),
    ];
    const source = `
      VStack {
        ForEach(Array(workspaces.enumerated()), id: \\.offset) { index, workspace in
          Text("\\(index): \\(workspace.title)")
        }
        ForEach(workspaces.dropFirst().prefix(1)) { workspace in
          Text("Next \\(workspace.title)")
        }
        for workspace in workspaces.reversed().suffix(1) {
          Text("Tail \\(workspace.title)")
        }
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\helpers.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("0: Ops lane");
    expect(markup).toContain("1: API lane");
    expect(markup).toContain("2: Docs lane");
    expect(markup).toContain('data-swift-id="0"');
    expect(markup).toContain('data-swift-id="1"');
    expect(markup).toContain('data-swift-id="2"');
    expect(markup).toContain("Next API lane");
    expect(markup).toContain("Tail Ops lane");
    expect(markup).not.toContain("Unsupported ForEach collection");
  });

  test("renders onTapGesture views as plain tappable buttons", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [];
    const source = `
      VStack {
        Text("Open Ops")
          .font(.headline)
          .onTapGesture {
            cmux("workspace.select", workspace_id: selectedId)
          }
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\tap.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("cmux-custom-sidebar-swift-button-plain");
    expect(markup).toContain("Open Ops");
    expect(markup).toContain("font-weight:760");
    expect(markup).not.toContain("No supported SwiftUI view found");
  });

  test("renders tap-count and long-press gestures as action affordances", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [];
    const source = `
      VStack {
        Text("Double")
          .onTapGesture(count: 2) {
            cmux("workspace.select", workspace_id: selectedId)
          }
        Text("Hold")
          .onLongPressGesture {
            cmux("sidebar.reload", name: sourceName)
          }
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\gestures.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("cmux-custom-sidebar-swift-button-multi-tap");
    expect(markup).toContain('data-swift-gesture="tap"');
    expect(markup).toContain('data-swift-tap-count="2"');
    expect(markup).toContain("data-tap-count=\"2\"");
    expect(markup).toContain("cmux-custom-sidebar-swift-button-long-press");
    expect(markup).toContain('data-swift-gesture="longPress"');
    expect(markup).toContain('data-swift-long-press-duration-ms="550"');
    expect(markup).toContain("Double");
    expect(markup).toContain("Hold");
    expect(markup).not.toContain("Unsupported Swift expression");
  });

  test("renders Swift Toggle bindings as read-only switch controls", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [
      workspace({
        workspace_id: "w1",
        custom_title: "Ops",
      }),
    ];
    const source = `
      VStack {
        Toggle("Selected \\(selectedTitle)", isOn: $workspaces[0].selected)
        Toggle(isOn: .constant(false)) {
          Text("Manual")
        }
        Toggle("Tap selected", isOn: workspaces[0].selected)
          .onTapGesture {
            cmux("workspace.select", workspace_id: selectedId)
          }
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\toggle.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain('role="switch"');
    expect(markup).toContain('aria-checked="true"');
    expect(markup).toContain('aria-checked="false"');
    expect(markup).toContain("cmux-custom-sidebar-swift-toggle-on");
    expect(markup).toContain("Selected Ops");
    expect(markup).toContain("Manual");
    expect(markup).toContain("cmux-custom-sidebar-swift-button-plain");
    expect(markup).not.toContain("Unsupported Swift view &#x27;Toggle&#x27;");
  });

  test("renders Swift TextField and Slider bindings as read-only controls", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [
      workspace({ workspace_id: "w1", custom_title: "Ops" }),
      workspace({ workspace_id: "w2", custom_title: "API" }),
    ];
    const source = `
      VStack {
        TextField("Selected workspace", text: $selectedTitle)
        TextField(text: .constant("Manual note")) {
          Text("Note")
        }
        Slider(value: $workspaceCount, in: 0...4) {
          Text("Workspace capacity")
        }
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\controls.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("cmux-custom-sidebar-swift-text-field");
    expect(markup).toContain('placeholder="Selected workspace"');
    expect(markup).toContain('value="Ops"');
    expect(markup).toContain('value="Manual note"');
    expect(markup).toContain("cmux-custom-sidebar-swift-slider");
    expect(markup).toContain('role="slider"');
    expect(markup).toContain('aria-valuemin="0"');
    expect(markup).toContain('aria-valuemax="4"');
    expect(markup).toContain('aria-valuenow="2"');
    expect(markup).toContain("width:50%");
    expect(markup).toContain("Workspace capacity");
    expect(markup).not.toContain("Unsupported Swift view &#x27;TextField&#x27;");
    expect(markup).not.toContain("Unsupported Swift view &#x27;Slider&#x27;");
  });

  test("renders Swift Form containers with editable controls", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [];
    const source = `
      @State var enabled = true
      @State var title = "Ops"

      Form {
        Section("Settings") {
          TextField("Title", text: $title)
          Toggle("Enabled", isOn: $enabled)
        }
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\form.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("cmux-custom-sidebar-swift-form");
    expect(markup).toContain('data-swift-form="true"');
    expect(markup).toContain("Settings");
    expect(markup).toContain('value="Ops"');
    expect(markup).toContain('role="switch"');
    expect(markup).toContain('aria-checked="true"');
    expect(markup).not.toContain("Unsupported Swift view &#x27;Form&#x27;");
  });

  test("renders Swift Picker bindings as read-only selection controls", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [
      workspace({ workspace_id: "w1", custom_title: "Ops" }),
      workspace({ workspace_id: "w2", custom_title: "API" }),
    ];
    const source = `
      Picker("Workspace", selection: $selectedTitle) {
        ForEach(workspaces) { workspace in
          Text(workspace.title)
        }
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\picker.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("cmux-custom-sidebar-swift-picker");
    expect(markup).toContain("cmux-custom-sidebar-swift-picker-value");
    expect(markup).toContain("Workspace");
    expect(markup).toContain("Ops");
    expect(markup).toContain("API");
    expect(markup).not.toContain("Unsupported Swift view &#x27;Picker&#x27;");
  });

  test("renders editable Picker tags as option values", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [];
    const source = `
      @State var lane = "api"

      Picker("Lane", selection: $lane) {
        Text("Operations").tag("ops")
        Text("API").tag("api")
        Text("Docs").tag("docs")
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\picker-tags.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("cmux-custom-sidebar-swift-picker-select");
    expect(markup).toContain('data-swift-state-binding="lane"');
    expect(markup).toContain('value="&quot;api&quot;"');
    expect(markup).toContain('data-swift-picker-tag="&quot;ops&quot;"');
    expect(markup).toContain('data-swift-picker-tag="&quot;api&quot;"');
    expect(markup).toContain("Operations");
    expect(markup).toContain("API");
    expect(markup).not.toContain("Unsupported Swift view &#x27;Picker&#x27;");
  });

  test("renders local @State-backed Swift controls as editable controls", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [];
    const source = `
      @State private var draftTitle = "Draft"
      @State var enabled = false
      @State var capacity = 2
      @State var lane = "Ops"
      @State var secret = "hunter2"
      @State var notes = "Line one"
      @State var count = 1
      @State var status = "idle"
      @State var rowFocused = false
      @State var due = "2026-07-09"
      @State var tint = "#2dd4bf"

      VStack {
        TextField("Draft title", text: $draftTitle)
          .onSubmit {
            status = "submitted"
            cmux("sidebar.reload", name: sourceName)
          }
          .onChange(of: draftTitle) {
            status = "changed"
          }
        SecureField("Token", text: $secret)
        TextEditor(text: $notes)
        Toggle("Enabled", isOn: $enabled)
        Slider(value: $capacity, in: 0...4)
        Stepper("Count", value: $count, in: 0...3)
        DatePicker("Due", selection: $due, displayedComponents: .date)
        ColorPicker("Tint", selection: $tint)
        Picker("Lane", selection: $lane) {
          Text("Ops")
          Text("API")
        }
        Text("Lifecycle")
          .onAppear {
            status = "appeared"
          }
          .onDisappear {
            status = "gone"
          }
        Text("Task")
          .task(id: sourceName) {
            status = "tasked"
          }
        Text("Hover")
          .onHover { isHovering in
            status = isHovering ? "hovering" : "idle"
          }
        Text("Focus")
          .focusable()
          .focused($rowFocused)
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\state.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("cmux-custom-sidebar-swift-control-editable");
    expect(markup).toContain('data-swift-state-binding="draftTitle"');
    expect(markup).toContain('data-swift-state-binding="enabled"');
    expect(markup).toContain('data-swift-state-binding="capacity"');
    expect(markup).toContain('data-swift-state-binding="lane"');
    expect(markup).toContain('data-swift-state-binding="secret"');
    expect(markup).toContain('data-swift-state-binding="notes"');
    expect(markup).toContain('data-swift-state-binding="count"');
    expect(markup).toContain('data-swift-state-binding="due"');
    expect(markup).toContain('data-swift-state-binding="tint"');
    expect(markup).toContain('data-swift-on-submit="true"');
    expect(markup).toContain('data-swift-on-change="true"');
    expect(markup).toContain('data-swift-on-appear="true"');
    expect(markup).toContain('data-swift-on-disappear="true"');
    expect(markup).toContain('data-swift-task="true"');
    expect(markup).toContain('data-swift-task-id="state.swift"');
    expect(markup).toContain('data-swift-task-id-expression="sourceName"');
    expect(markup).toContain('data-swift-on-hover="true"');
    expect(markup).toContain("cmux-custom-sidebar-swift-hoverable");
    expect(markup).toContain('data-swift-focusable="true"');
    expect(markup).toContain('data-swift-focused="false"');
    expect(markup).toContain('data-swift-focused-binding="rowFocused"');
    expect(markup).toContain("cmux-custom-sidebar-swift-focusable");
    expect(markup).toContain('value="Draft"');
    expect(markup).toContain('type="password"');
    expect(markup).toContain("cmux-custom-sidebar-swift-text-editor-input");
    expect(markup).toContain("cmux-custom-sidebar-swift-stepper");
    expect(markup).toContain("cmux-custom-sidebar-swift-date-picker");
    expect(markup).toContain("cmux-custom-sidebar-swift-color-picker");
    expect(markup).toContain('role="spinbutton"');
    expect(markup).toContain('type="date"');
    expect(markup).toContain('type="color"');
    expect(markup).toContain('tabindex="0"');
    expect(markup).toContain('type="range"');
    expect(markup).toContain("cmux-custom-sidebar-swift-picker-select");
    expect(markup).not.toContain("readonly");
    expect(markup).not.toContain("Unsupported @State binding");
  });

  test("renders ForEach collection element bindings as editable controls", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [];
    const source = `
      @State var tasks = [
        ["title": "Draft", "done": false, "points": 1],
        ["title": "Review", "done": true, "points": 2]
      ]

      VStack {
        ForEach($tasks) { $task in
          TextField("Task", text: $task.title)
          Toggle("Done", isOn: $task.done)
          Stepper("Points", value: $task.points, in: 0...5)
        }
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\task-bindings.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain('data-swift-state-binding="tasks[0].title"');
    expect(markup).toContain('data-swift-state-binding="tasks[0].done"');
    expect(markup).toContain('data-swift-state-binding="tasks[0].points"');
    expect(markup).toContain('data-swift-state-binding="tasks[1].title"');
    expect(markup).toContain('value="Draft"');
    expect(markup).toContain('value="Review"');
    expect(markup).toContain("cmux-custom-sidebar-swift-control-editable");
    expect(markup).not.toContain("Unsupported ForEach collection");
    expect(markup).not.toContain("Unsupported @State binding");
  });

  test("renders Swift local bindings and user helper functions", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [
      workspace({ workspace_id: "w1", custom_title: "Ops" }),
      workspace({ workspace_id: "w2", custom_title: "API" }),
    ];
    const source = `
      let footer = selectedTitle

      func marker(_ workspace: WorkspacePreview) -> String {
        return workspace.selected ? "●" : "○"
      }

      func row(_ workspace: WorkspacePreview) -> some View {
        HStack {
          Text(marker(workspace))
          Text(workspace.title)
        }
      }

      VStack {
        ForEach(workspaces) { workspace in
          row(workspace)
        }
        Text(footer)
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\helpers.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("Ops");
    expect(markup).toContain("API");
    expect(markup).toContain("●");
    expect(markup).toContain("○");
    expect(markup).not.toContain("Unsupported Swift view &#x27;row&#x27;");
    expect(markup).not.toContain("Unsupported Swift expression");
  });

  test("renders constrained Swift environment reads", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [];
    const source = `
      @Environment(\\.colorScheme) var colorScheme
      @Environment(\\.layoutDirection) var layoutDirection
      @Environment(\\.locale) var locale

      VStack {
        Text("Scheme \\(colorScheme)")
        if colorScheme == .light {
          Text("Light branch")
        }
        if layoutDirection == .leftToRight {
          Text("LTR branch")
        }
        Text("Locale \\(locale.identifier)")
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\environment.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("Scheme light");
    expect(markup).toContain("Light branch");
    expect(markup).toContain("LTR branch");
    expect(markup).toContain("Locale en-US");
    expect(markup).not.toContain("Unsupported @Environment binding");
    expect(markup).not.toContain("Unsupported @Environment key");
    expect(markup).not.toContain("Unsupported Swift expression");
  });

  test("renders Swift string helpers and numeric builtins", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [workspace({ workspace_id: "w1", custom_title: "Ops" })];
    const source = `
      VStack {
        if selectedTitle.hasPrefix("O") {
          Text("\\(selectedTitle.uppercased()) \\(String(max(workspaceCount, 3)))")
        }
        ForEach(selectedTitle.split(separator: "p")) { part in
          Text(part.lowercased())
        }
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\value-helpers.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("OPS 3");
    expect(markup).toContain(">o<");
    expect(markup).toContain(">s<");
    expect(markup).not.toContain("Unsupported ForEach collection");
    expect(markup).not.toContain("Unsupported Swift expression");
  });

  test("renders Swift Text markdown and verbatim literals", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [workspace({ workspace_id: "w1", custom_title: "Ops" })];
    const source = `
      VStack {
        Text("**Ops** uses \`cmux\`, *fast*, and [docs](https://example.com)")
        Text(markdown: "[unsafe](javascript:alert(1))")
        Text(verbatim: "**literal**")
        Text("**\\(selectedTitle)**")
        Text("Open ") + Text(selectedTitle)
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\text-markdown.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("<strong>Ops</strong>");
    expect(markup).toContain("<code>cmux</code>");
    expect(markup).toContain("<em>fast</em>");
    expect(markup).toContain('href="https://example.com"');
    expect(markup).toContain('href="#"');
    expect(markup).toContain("**literal**");
    expect(markup).toContain("**Ops**");
    expect(markup).toContain("Open Ops");
    expect(markup.match(/<strong>Ops<\/strong>/g) ?? []).toHaveLength(1);
    expect(markup).not.toContain("javascript:alert");
    expect(markup).not.toContain("Unsupported Swift view");
  });

  test("renders Swift Link views with safe external destinations", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [workspace({ workspace_id: "w1", custom_title: "Ops" })];
    const source = `
      VStack {
        Link("Docs", destination: URL(string: "https://example.com/docs")!)
        Link(destination: URL(string: "https://example.com/pr")) {
          Label("Pull request", systemImage: "arrow.up.right")
        }
        Link("Unsafe", destination: URL(string: "file:///tmp/secret")!)
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\links.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("cmux-custom-sidebar-swift-link");
    expect(markup).toContain('href="https://example.com/docs"');
    expect(markup).toContain('href="https://example.com/pr"');
    expect(markup).toContain('target="_blank"');
    expect(markup).toContain('rel="noreferrer"');
    expect(markup).toContain('data-swift-link="Docs"');
    expect(markup).toContain('data-swift-link="Pull request"');
    expect(markup).toContain('data-swift-link-destination="https://example.com/pr"');
    expect(markup).toContain('data-swift-system-image="arrow.up.right"');
    expect(markup).toContain("cmux-custom-sidebar-swift-link-disabled");
    expect(markup).toContain('data-swift-link-blocked="true"');
    expect(markup).toContain('role="link"');
    expect(markup).toContain('aria-disabled="true"');
    expect(markup).not.toContain("file:///tmp/secret");
    expect(markup).not.toContain("Unsupported Swift view");
  });

  test("renders Swift ContentUnavailableView empty states", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [];
    const source = `
      VStack {
        ContentUnavailableView(
          "No Workspaces",
          systemImage: "tray",
          description: Text("Create a workspace to begin")
        )
        ContentUnavailableView {
          Label("No Results", systemImage: "magnifyingglass")
        } description: {
          Text("Try another filter")
        } actions: {
          Button("Reload") { cmux("sidebar.reload", name: sourceName) }
        }
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\empty.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("cmux-custom-sidebar-swift-content-unavailable");
    expect(markup).toContain("cmux-custom-sidebar-swift-content-unavailable-icon");
    expect(markup).toContain("cmux-custom-sidebar-swift-content-unavailable-label");
    expect(markup).toContain("cmux-custom-sidebar-swift-content-unavailable-description");
    expect(markup).toContain("cmux-custom-sidebar-swift-content-unavailable-actions");
    expect(markup).toContain('data-swift-content-unavailable="No Workspaces"');
    expect(markup).toContain('data-swift-content-unavailable="No Results"');
    expect(markup).toContain('data-swift-system-image="tray"');
    expect(markup).toContain('data-swift-system-image="magnifyingglass"');
    expect(markup).toContain('data-swift-system-image-glyph="tray"');
    expect(markup).toContain("Create a workspace to begin");
    expect(markup).toContain("Try another filter");
    expect(markup).toContain("Reload");
    expect(markup).not.toContain("Unsupported Swift view");
  });

  test("renders Swift arithmetic, comparison, and logical expressions", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [
      workspace({ workspace_id: "w1", custom_title: "Ops" }),
      workspace({ workspace_id: "w2", custom_title: "API" }),
    ];
    const source = `
      VStack {
        if workspaceCount * 2 == 4 && !workspaces[1].selected {
          Text("\\((workspaceCount + 2) * 5)")
        }
        Text("Lane " + selectedTitle)
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\operators.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain(">20<");
    expect(markup).toContain("Lane Ops");
    expect(markup).not.toContain("Unsupported Swift expression");
  });

  test("renders Swift dictionary literals and keyed subscripts", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [workspace({ workspace_id: "w1", custom_title: "Ops" })];
    const source = `
      let labels = [selectedId: selectedTitle, "fallback": "Idle"]
      VStack {
        Text(labels[workspaces[0].id])
        Text(labels["fallback"])
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\dictionaries.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain(">Ops<");
    expect(markup).toContain(">Idle<");
    expect(markup).not.toContain("Unsupported Swift expression");
  });

  test("renders Swift collection transform closures", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [
      workspace({
        workspace_id: "w1",
        custom_title: "Ops",
        panel_unreads: [{ panel_id: "surface-1", is_unread: true }],
      }),
      workspace({
        workspace_id: "w2",
        custom_title: "API",
        panel_unreads: [
          { panel_id: "surface-2", is_unread: true },
          { panel_id: "surface-3", is_unread: true },
        ],
      }),
    ];
    const source = `
      let grouped = Dictionary(grouping: workspaces, by: \\.selected)
      VStack {
        ForEach(workspaces.filter { $0.selected }) { workspace in
          Text("Selected \\(workspace.title)")
        }
        ForEach(workspaces.sorted { $0.title < $1.title }) { workspace in
          Text("Sorted \\(workspace.title)")
        }
        ForEach(workspaces.map(\\.title), id: \\.self) { title in
          Text("Mapped \\(title)")
        }
        ForEach(workspaces.sorted(by: \\.title)) { workspace in
          Text("KeySorted \\(workspace.title)")
        }
        Text(String(workspaces.compactMap(\\.progress).count))
        Text("Least unread \\(workspaces.min(by: \\.unreadCount).title)")
        Text("Most unread \\(workspaces.max(by: \\.unreadCount).title)")
        Text("All selected \\(String(workspaces.allSatisfy(\\.selected)))")
        Text("Selected group \\(grouped[true].count)")
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\transforms.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("Selected Ops");
    expect(markup.indexOf("Sorted API")).toBeLessThan(markup.indexOf("Sorted Ops"));
    expect(markup).toContain("Mapped Ops");
    expect(markup).toContain("Mapped API");
    expect(markup.indexOf("KeySorted API")).toBeLessThan(markup.indexOf("KeySorted Ops"));
    expect(markup).toContain(">0<");
    expect(markup).toContain("Least unread Ops");
    expect(markup).toContain("Most unread API");
    expect(markup).toContain("All selected false");
    expect(markup).toContain("Selected group 1");
    expect(markup).not.toContain("Unsupported ForEach collection");
    expect(markup).not.toContain("Unsupported Swift expression");
  });

  test("renders Swift reduce and numeric formatting helpers", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [
      workspace({
        workspace_id: "w1",
        custom_title: "Ops",
        panel_unreads: [{ panel_id: "surface-1", is_unread: true }],
      }),
      workspace({
        workspace_id: "w2",
        custom_title: "API",
        panel_unreads: [
          { panel_id: "surface-2", is_unread: true },
          { panel_id: "surface-3", is_unread: true },
        ],
        listening_ports: Array.from({ length: 1199 }, (_entry, index) => index + 1),
      }),
    ];
    const source = `
      let totalUnread = workspaces.reduce(0) { total, workspace in
        total + workspace.unreadCount
      }
      let ratio = 0.25
      let bytes = 1536
      let distance = Measurement(value: 12.5, unit: UnitLength.kilometers)
      let launched = Date(timeIntervalSince1970: 1704067200)
      let intervalEnd = Date(timeIntervalSince1970: 1704070800)
      let names = ["Ops", "API", "Docs"]
      VStack {
        Text(String(totalUnread))
        Text(ratio.formatted(.percent))
        Text(portTotal, format: .notation(.compactName))
        Text(String(format: "%03d %@", arguments: [workspaceCount, selectedTitle]))
        Text(bytes.formatted(.byteCount(style: .file)))
        Text(distance.formatted(.measurement(width: .abbreviated)))
        Text(launched.formatted(.dateTime.year().month().day()))
        Text(launched, style: .date)
        Text(launched, style: .time)
        Text(timerInterval: launched...intervalEnd)
        Text(names.formatted(.list(type: .and)))
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\formatting.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain(">3<");
    expect(markup).toContain(">25%<");
    expect(markup).toContain(">1.2K<");
    expect(markup).toContain(">002 Ops<");
    expect(markup).toContain(">1.5 KB<");
    expect(markup).toContain(">12.5 km<");
    expect(markup).toContain(">Jan 1, 2024<");
    expect(markup).toContain('data-swift-text-style="date"');
    expect(markup).toContain('data-swift-text-style="time"');
    expect(markup).toContain('data-swift-text-style="timer"');
    expect(markup).toContain('data-swift-timer-interval-start-ms="1704067200000"');
    expect(markup).toContain('data-swift-timer-interval-end-ms="1704070800000"');
    expect(markup).toContain('data-swift-timer-counts-down="true"');
    expect(markup).toContain(">1:00:00<");
    expect(markup).toContain(">Ops, API, and Docs<");
    expect(markup).not.toContain("Unsupported Swift expression");
  });

  test("renders scroll views, lazy stack metadata, button roles, and list row modifiers", () => {
    currentSelectedWorkspaceIndex = 0;
    currentWorkspaces = [];
    const source = `
      ScrollView(.horizontal, showsIndicators: false) {
        LazyHStack(alignment: .top, spacing: 14, pinnedViews: [.sectionHeaders]) {
          Button("Delete", role: .destructive) {
            cmux("workspace.set_status", key: "delete", value: "armed")
          }
            .listRowBackground(.blue)
            .listRowSeparator(.hidden)
          Button("Cancel", role: .cancel) {
            cmux("workspace.clear_status", key: "delete")
          }
        }
      }
      ScrollView([.horizontal, .vertical], showsIndicators: true) {
        Text("Pan")
      }
    `;

    const markup = renderToStaticMarkup(
      <CustomSidebarSurface
        sourcePath="C:\\Users\\User\\.config\\cmux\\sidebars\\scroll.swift"
        sourceOverride={source}
      />,
    );

    expect(markup).toContain("cmux-custom-sidebar-swift-scroll-horizontal");
    expect(markup).toContain("cmux-custom-sidebar-swift-scroll-no-indicators");
    expect(markup).toContain('data-swift-scroll-axis="horizontal"');
    expect(markup).toContain('data-swift-scroll-shows-indicators="false"');
    expect(markup).toContain("cmux-custom-sidebar-swift-scroll-both");
    expect(markup).toContain('data-swift-scroll-axis="both"');
    expect(markup).toContain('data-swift-scroll-shows-indicators="true"');
    expect(markup).toContain(">Pan<");
    expect(markup).toContain("cmux-custom-sidebar-swift-stack-lazy");
    expect(markup).toContain("cmux-custom-sidebar-swift-pinned-sectionHeaders");
    expect(markup).toContain('data-swift-pinned-views="sectionHeaders"');
    expect(markup).toContain("cmux-custom-sidebar-swift-stack-alignment-top");
    expect(markup).toContain("gap:14px");
    expect(markup).toContain("align-items:flex-start");
    expect(markup).toContain("cmux-custom-sidebar-swift-button-destructive");
    expect(markup).toContain("cmux-custom-sidebar-swift-button-cancel");
    expect(markup).toContain("cmux-custom-sidebar-swift-list-row-separator-hidden");
    expect(markup).toContain("background:rgba(59, 130, 246, 0.14)");
  });
});
