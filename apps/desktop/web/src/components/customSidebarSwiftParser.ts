import {
  type CustomSidebarJsonAction,
  type CustomSidebarSwiftDocument,
  type CustomSidebarSwiftEventHandler,
  type CustomSidebarSwiftLocalHandler,
  type CustomSidebarSwiftLocalHandlerModifierName,
  type CustomSidebarSwiftModifier,
  type CustomSidebarSwiftNode,
  type CustomSidebarSwiftPickerOption,
  type CustomSidebarSwiftStateAssignment,
  type CustomSidebarSwiftTextRun,
  emptyCustomSidebarEventsContext,
  isSwiftGridItem,
  type JsonTemplateContext,
  safeCustomSidebarAssetUrl,
  type SwiftAngleValue,
  type SwiftDateIntervalValue,
  type SwiftDateValue,
  type SwiftGridItemValue,
  type SwiftMeasurementValue,
  type SwiftPointValue,
  type SwiftRectValue,
  type SwiftSidebarStateValue,
  type SwiftSizeValue,
  type SwiftUnitPointValue,
  type TabPreview,
  type TemplateContext,
  type WorkspacePreview,
} from "./customSidebarModel";

export interface SwiftParseScope {
  root: JsonTemplateContext & { clock: { time: string }; workspaces: WorkspacePreview[] };
  workspace?: WorkspacePreview;
  tab?: TabPreview;
  __functions?: Record<string, SwiftUserFunction>;
  __stateValues?: Record<string, SwiftSidebarStateValue>;
  __stateKeys?: Record<string, true>;
  __bindingKeys?: Record<string, string>;
  [key: string]: unknown;
}

export interface SwiftUserFunction {
  params: string[];
  body: string;
  returnsView: boolean;
}

export class SwiftSidebarParser {
  readonly warnings: string[] = [];

  constructor(
    private readonly source: string,
    private readonly scope: SwiftParseScope,
  ) {}

  parse(): CustomSidebarSwiftDocument {
    const nodes = this.parseStatements(this.source, this.scope);
    const root: CustomSidebarSwiftNode =
      nodes.length === 1
        ? nodes[0] ?? { kind: "group", children: [] }
        : { kind: "group", children: nodes };
    const eventHandlers = this.collectEventHandlers(root);
    if (nodes.length === 1) {
      return { root, warnings: this.warnings, eventHandlers };
    }
    return { root, warnings: this.warnings, eventHandlers };
  }

  private collectEventHandlers(node: CustomSidebarSwiftNode): CustomSidebarSwiftEventHandler[] {
    const handlers = [...(node.eventHandlers ?? [])];
    switch (node.kind) {
      case "modified":
        handlers.push(...this.collectEventHandlers(node.base));
        for (const modifier of node.childModifiers) {
          for (const child of modifier.children ?? []) {
            handlers.push(...this.collectEventHandlers(child));
          }
        }
        break;
      case "vstack":
      case "hstack":
      case "zstack":
      case "group":
      case "splitView":
      case "navigationStack":
      case "navigationLink":
      case "tabView":
      case "scrollView":
      case "list":
      case "section":
      case "groupBox":
      case "disclosureGroup":
      case "grid":
      case "gridRow":
      case "menu":
      case "button":
      case "textField":
      case "stepper":
      case "slider":
      case "picker":
      case "datePicker":
      case "colorPicker":
      case "toggle":
        for (const child of node.children) {
          handlers.push(...this.collectEventHandlers(child));
        }
        if (node.kind === "navigationLink") {
          for (const child of node.destination) {
            handlers.push(...this.collectEventHandlers(child));
          }
        }
        if (node.kind === "section") {
          for (const child of node.header ?? []) {
            handlers.push(...this.collectEventHandlers(child));
          }
          for (const child of node.footer ?? []) {
            handlers.push(...this.collectEventHandlers(child));
          }
        }
        if (node.kind === "groupBox" || node.kind === "disclosureGroup") {
          for (const child of node.labelChildren) {
            handlers.push(...this.collectEventHandlers(child));
          }
        }
        break;
      case "externalLink":
        for (const child of node.labelChildren) {
          handlers.push(...this.collectEventHandlers(child));
        }
        break;
      case "contentUnavailable":
        for (const child of [
          ...node.labelChildren,
          ...node.descriptionChildren,
          ...node.actionsChildren,
        ]) {
          handlers.push(...this.collectEventHandlers(child));
        }
        break;
      case "asyncImage":
        for (const child of node.successChildren ?? []) {
          handlers.push(...this.collectEventHandlers(child));
        }
        for (const child of node.placeholderChildren ?? []) {
          handlers.push(...this.collectEventHandlers(child));
        }
        break;
      default:
        break;
    }
    return handlers;
  }

  private parseViewExpression(
    expression: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode | null {
    const text = expression.trim();
    if (!text) return null;
    const textConcatenation = this.parseTextConcatenation(text, scope);
    if (textConcatenation !== null) return textConcatenation;
    const call = this.readViewCall(text);
    if (call === null) {
      this.warnings.push(`Unsupported Swift expression: ${text.slice(0, 40)}`);
      return null;
    }
    const asyncTrailing =
      call.name === "AsyncImage" ||
      call.name === "GroupBox" ||
      call.name === "DisclosureGroup" ||
      call.name === "ContentUnavailableView"
        ? this.readTrailingClosureSequence(text.slice(call.end))
        : null;
    const trailing =
      asyncTrailing?.end === undefined || asyncTrailing.end === 0
        ? this.readTrailingClosure(text.slice(call.end))
        : {
            body: asyncTrailing.first ?? "",
            end: asyncTrailing.end,
          };
    const decorate = (node: CustomSidebarSwiftNode): CustomSidebarSwiftNode =>
      this.withModifiers(node, text, call.end + (trailing?.end ?? 0), scope);
    switch (call.name) {
      case "VStack":
      case "LazyVStack":
        return decorate({
          kind: "vstack",
          alignment: this.swiftAlignmentToken(this.namedArg(call.args, "alignment"), scope),
          spacing: this.stackSpacing(call.args, scope),
          ...(call.name === "LazyVStack" ? { lazyStack: true } : {}),
          ...(call.name === "LazyVStack"
            ? { pinnedViews: this.swiftPinnedViews(this.namedArg(call.args, "pinnedViews"), scope) }
            : {}),
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "HStack":
      case "LazyHStack":
        return decorate({
          kind: "hstack",
          alignment: this.swiftAlignmentToken(this.namedArg(call.args, "alignment"), scope),
          spacing: this.stackSpacing(call.args, scope),
          ...(call.name === "LazyHStack" ? { lazyStack: true } : {}),
          ...(call.name === "LazyHStack"
            ? { pinnedViews: this.swiftPinnedViews(this.namedArg(call.args, "pinnedViews"), scope) }
            : {}),
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "HSplitView":
        return decorate({
          kind: "splitView",
          axis: "horizontal",
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "VSplitView":
        return decorate({
          kind: "splitView",
          axis: "vertical",
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "NavigationStack":
        return decorate({
          kind: "navigationStack",
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "NavigationLink":
        return decorate(this.parseNavigationLink(call.args, trailing?.body ?? "", scope));
      case "Link":
        return decorate(this.parseExternalLink(call.args, trailing?.body ?? "", scope));
      case "ContentUnavailableView":
        return decorate(this.parseContentUnavailableView(
          call.args,
          trailing?.body ?? "",
          scope,
          asyncTrailing?.labeled,
        ));
      case "TabView":
        return decorate({
          kind: "tabView",
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "ZStack":
        return decorate({
          kind: "zstack",
          alignment: this.swiftAlignmentToken(this.namedArg(call.args, "alignment"), scope),
          spacing: this.stackSpacing(call.args, scope),
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "Group":
        return decorate({
          kind: "group",
          groupRole: "group",
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "ToolbarItem":
        return decorate({
          kind: "group",
          groupRole: "toolbarItem",
          toolbarPlacement: this.swiftTypeToken(this.namedArg(call.args, "placement")),
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "ControlGroup":
        return decorate({
          kind: "group",
          groupRole: "controlGroup",
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "ViewThatFits":
        return decorate({
          kind: "group",
          groupRole: "viewThatFits",
          fitAxis: this.scrollAxis(
            this.namedArg(call.args, "in") ?? this.firstPositionalArg(call.args, scope),
          ),
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "AnyView": {
        const node = this.parseViewExpression(call.args, scope);
        return node === null ? null : decorate(node);
      }
      case "ScrollView":
        return decorate(this.parseScrollView(call.args, trailing?.body ?? "", scope));
      case "List":
        return decorate(this.parseList(call.args, trailing?.body ?? "", scope));
      case "Form":
        return decorate({
          kind: "list",
          form: true,
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "Section":
        return decorate(this.parseSection(call.args, trailing?.body ?? "", scope));
      case "Grid":
      case "LazyVGrid":
      case "LazyHGrid":
        return decorate(this.parseGrid(call.name, call.args, trailing?.body ?? "", scope));
      case "GridRow":
        return decorate({
          kind: "gridRow",
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "Menu":
        return decorate({
          kind: "menu",
          title: this.firstPositionalArg(call.args, scope),
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "Text":
        return decorate(this.parseText(call.args, scope));
      case "Label":
        return decorate(this.parseLabel(call.args, scope));
      case "LabeledContent":
        return decorate(this.parseLabeledContent(call.args, trailing?.body ?? "", scope));
      case "GroupBox":
        return decorate(this.parseGroupBox(
          call.args,
          trailing?.body ?? "",
          scope,
          asyncTrailing?.labeled,
        ));
      case "DisclosureGroup":
        return decorate(this.parseDisclosureGroup(
          call.args,
          trailing?.body ?? "",
          scope,
          asyncTrailing?.labeled,
        ));
      case "Image":
        return decorate(this.parseImage(call.args, scope));
      case "AsyncImage":
        return decorate(this.parseAsyncImage(
          call.args,
          trailing?.body ?? "",
          scope,
          asyncTrailing?.labeled,
        ));
      case "ProgressView":
        return decorate(this.parseProgressView(call.args, scope));
      case "Gauge":
        return decorate(this.parseGauge(call.args, scope));
      case "TextField":
        return decorate(this.parseTextField(call.args, trailing?.body ?? "", scope));
      case "SecureField":
        return decorate(this.parseTextField(call.args, trailing?.body ?? "", scope, {
          secure: true,
        }));
      case "TextEditor":
        return decorate(this.parseTextField(call.args, trailing?.body ?? "", scope, {
          multiline: true,
        }));
      case "Stepper":
        return decorate(this.parseStepper(call.args, trailing?.body ?? "", scope));
      case "Slider":
        return decorate(this.parseSlider(call.args, trailing?.body ?? "", scope));
      case "Picker":
        return decorate(this.parsePicker(call.args, trailing?.body ?? "", scope));
      case "DatePicker":
        return decorate(this.parseDatePicker(call.args, trailing?.body ?? "", scope));
      case "ColorPicker":
        return decorate(this.parseColorPicker(call.args, trailing?.body ?? "", scope));
      case "Toggle":
        return decorate(this.parseToggle(call.args, trailing?.body ?? "", scope));
      case "Circle":
        return decorate({ kind: "shape", shape: "circle" });
      case "Ellipse":
        return decorate({ kind: "shape", shape: "ellipse" });
      case "Rectangle":
        return decorate({ kind: "shape", shape: "rectangle" });
      case "RoundedRectangle":
        return decorate({
          kind: "shape",
          shape: "roundedRectangle",
          radius: this.toNumber(this.namedArg(call.args, "cornerRadius"), scope),
          cornerStyle: this.swiftRoundedCornerStyleToken(this.namedArg(call.args, "style"), scope),
        });
      case "UnevenRoundedRectangle":
        return decorate({
          kind: "shape",
          shape: "unevenRoundedRectangle",
          radius: this.toNumber(this.namedArg(call.args, "cornerRadius"), scope),
        });
      case "Capsule":
        return decorate({
          kind: "shape",
          shape: "capsule",
          cornerStyle: this.swiftRoundedCornerStyleToken(this.namedArg(call.args, "style"), scope),
        });
      case "ContainerRelativeShape":
        return decorate({ kind: "shape", shape: "containerRelativeShape" });
      case "Path":
        return decorate(this.parsePath(call.args, scope));
      case "Divider":
        return decorate({ kind: "divider" });
      case "Spacer":
        return decorate({
          kind: "spacer",
          minLength: this.toNumber(this.namedArg(call.args, "minLength"), scope),
        });
      case "EmptyView":
        return decorate({ kind: "empty" });
      case "Button":
        return decorate(this.parseButton(call.args, trailing?.body ?? "", scope));
      default:
        if (this.userFunction(call.name, scope)?.returnsView === true) {
          return decorate(this.invokeViewFunction(call.name, call.args, scope));
        }
        this.warnings.push(`Unsupported Swift view '${call.name}'`);
        return null;
    }
  }

  private parseTextConcatenation(
    text: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode | null {
    const index = this.topLevelOperatorIndex(text, ["+"], true);
    if (index < 0) return null;
    const left = this.parseViewExpression(text.slice(0, index), scope);
    const right = this.parseViewExpression(text.slice(index + 1), scope);
    if (left?.kind !== "text" || right?.kind !== "text") {
      this.warnings.push(`Unsupported Swift Text concatenation: ${text.slice(0, 48)}`);
      return null;
    }
    return {
      kind: "text",
      text: `${left.text}${right.text}`,
      ...(left.markdownRuns !== undefined || right.markdownRuns !== undefined
        ? {
            markdownRuns: [
              ...(left.markdownRuns ?? [{ kind: "text" as const, text: left.text }]),
              ...(right.markdownRuns ?? [{ kind: "text" as const, text: right.text }]),
            ],
          }
        : {}),
    };
  }

  private parseStatements(body: string, scope: SwiftParseScope): CustomSidebarSwiftNode[] {
    const nodes: CustomSidebarSwiftNode[] = [];
    let index = 0;
    let currentScope = scope;
    while (index < body.length && nodes.length < 250) {
      index = this.skipWhitespaceAndSeparators(body, index);
      if (index >= body.length) break;
      if (body.startsWith("@State", index)) {
        const parsed = this.parseStateBinding(body, index, currentScope);
        currentScope = parsed.scope;
        index = parsed.end;
        continue;
      }
      if (body.startsWith("@Environment", index)) {
        const parsed = this.parseEnvironmentBinding(body, index, currentScope);
        currentScope = parsed.scope;
        index = parsed.end;
        continue;
      }
      if (body.startsWith("let ", index) || body.startsWith("let\t", index)) {
        const parsed = this.parseLetBinding(body, index, currentScope);
        currentScope = parsed.scope;
        index = parsed.end;
        continue;
      }
      if (body.startsWith("func ", index) || body.startsWith("func\t", index)) {
        const parsed = this.parseFunctionDeclaration(body, index, currentScope);
        currentScope = parsed.scope;
        index = parsed.end;
        continue;
      }
      if (body.startsWith("return ", index) || body.startsWith("return\t", index)) {
        const statement = this.readViewStatement(
          body,
          this.skipWhitespaceAndSeparators(body, index + "return".length),
        );
        if (statement === null) break;
        const node = this.parseViewExpression(statement.text, currentScope);
        if (node !== null) nodes.push(node);
        break;
      }
      if (body.startsWith("ForEach", index)) {
        const parsed = this.parseForEach(body, index, currentScope);
        nodes.push(...parsed.nodes);
        index = parsed.end;
        continue;
      }
      if (body.startsWith("for ", index) || body.startsWith("for\t", index)) {
        const parsed = this.parseForLoop(body, index, currentScope);
        nodes.push(...parsed.nodes);
        index = parsed.end;
        continue;
      }
      if (body.startsWith("if ", index) || body.startsWith("if\t", index)) {
        const parsed = this.parseIf(body, index, currentScope);
        nodes.push(...parsed.nodes);
        index = parsed.end;
        continue;
      }
      const statement = this.readViewStatement(body, index);
      if (statement === null) {
        index += 1;
        continue;
      }
      const node = this.parseViewExpression(statement.text, currentScope);
      if (node !== null) nodes.push(node);
      index = statement.end;
    }
    return nodes;
  }

  private parseStateBinding(
    body: string,
    index: number,
    scope: SwiftParseScope,
  ): { scope: SwiftParseScope; end: number } {
    const statement = this.readSimpleStatement(body, index);
    const match = statement.text.match(
      /^@State\s+(?:(?:private|fileprivate|internal|public)\s+)?var\s+([A-Za-z_][A-Za-z0-9_]*)(?:\s*:\s*[^=]+)?\s*=\s*([\s\S]+)$/,
    );
    if (!match) {
      this.warnings.push(`Unsupported @State binding '${statement.text.slice(0, 48)}'`);
      return { scope, end: statement.end };
    }
    const key = match[1] ?? "value";
    const defaultValue = this.swiftStateValue(this.evalValue(match[2] ?? "", scope));
    const stateValue =
      scope.__stateValues !== undefined &&
      Object.prototype.hasOwnProperty.call(scope.__stateValues, key)
        ? scope.__stateValues[key]
        : defaultValue;
    return {
      scope: {
        ...scope,
        [key]: stateValue,
        __stateKeys: { ...(scope.__stateKeys ?? {}), [key]: true },
      },
      end: statement.end,
    };
  }

  private parseEnvironmentBinding(
    body: string,
    index: number,
    scope: SwiftParseScope,
  ): { scope: SwiftParseScope; end: number } {
    const statement = this.readSimpleStatement(body, index);
    const match = statement.text.match(
      /^@Environment\s*\(([\s\S]+?)\)\s+(?:(?:private|fileprivate|internal|public)\s+)?var\s+([A-Za-z_][A-Za-z0-9_]*)(?:\s*:\s*.+)?$/,
    );
    if (!match) {
      this.warnings.push(`Unsupported @Environment binding '${statement.text.slice(0, 48)}'`);
      return { scope, end: statement.end };
    }
    const key = this.swiftEnvironmentReadKeyToken(match[1], scope);
    if (key === undefined) {
      this.warnings.push(`Unsupported @Environment key '${(match[1] ?? "").trim()}'`);
      return { scope, end: statement.end };
    }
    return {
      scope: {
        ...scope,
        [match[2] ?? key]: this.swiftEnvironmentReadValue(key),
      } as SwiftParseScope,
      end: statement.end,
    };
  }

  private parseLetBinding(
    body: string,
    index: number,
    scope: SwiftParseScope,
  ): { scope: SwiftParseScope; end: number } {
    const statement = this.readSimpleStatement(body, index);
    const match = statement.text.match(/^let\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*([\s\S]+)$/);
    if (!match) {
      this.warnings.push(`Unsupported let binding '${statement.text.slice(0, 40)}'`);
      return { scope, end: statement.end };
    }
    return {
      scope: {
        ...scope,
        [match[1] ?? "value"]: this.evalValue(match[2] ?? "", scope),
      } as SwiftParseScope,
      end: statement.end,
    };
  }

  private parseFunctionDeclaration(
    body: string,
    index: number,
    scope: SwiftParseScope,
  ): { scope: SwiftParseScope; end: number } {
    const source = body.slice(index);
    const match = source.match(/^func\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(/);
    if (!match) return { scope, end: index + 4 };
    const open = index + source.indexOf("(");
    const params = this.readBalanced(body, open, "(", ")");
    if (params === null) return { scope, end: open + 1 };
    const brace = body.indexOf("{", params.end);
    if (brace < 0) return { scope, end: params.end };
    const block = this.readBalanced(body, brace, "{", "}");
    if (block === null) return { scope, end: brace + 1 };
    const returnSignature = body.slice(params.end, brace);
    const name = match[1] ?? "";
    return {
      scope: {
        ...scope,
        __functions: {
          ...(scope.__functions ?? {}),
          [name]: {
            params: this.parseFunctionParams(params.content),
            body: block.content,
            returnsView: /\bsome\s+View\b|\bView\b/.test(returnSignature),
          },
        },
      } as SwiftParseScope,
      end: block.end,
    };
  }

  private parseForEach(
    body: string,
    index: number,
    scope: SwiftParseScope,
  ): { nodes: CustomSidebarSwiftNode[]; end: number } {
    const call = this.readCall(body.slice(index));
    if (call === null) return { nodes: [], end: index + 1 };
    const trailing = this.readTrailingClosure(body.slice(index + call.end));
    if (trailing === null) return { nodes: [], end: index + call.end };
    const collection = this.splitTopLevel(call.args, ",")[0]?.trim();
    const idExpression = this.namedArg(call.args, "id");
    const bindingCollectionKey = this.stateBindingKey(collection ?? "", scope);
    const values = this.evalSequence(bindingCollectionKey ?? collection ?? "", scope);
    if (values === null) {
      this.warnings.push(`Unsupported ForEach collection '${collection ?? ""}'`);
      return { nodes: [], end: index + call.end + trailing.end };
    }
    const closure = this.splitClosureParameter(trailing.body);
    const params = closure.params.length > 0 ? closure.params : ["workspace"];
    const nodes = values.flatMap((value, valueIndex) =>
      this.applyForEachIdentity(
        this.parseStatements(
          closure.body,
          this.scopeWithForEachValues(scope, params, value, bindingCollectionKey, valueIndex),
        ),
        this.forEachIdentityValue(idExpression, value),
      ),
    );
    return { nodes, end: index + call.end + trailing.end };
  }

  private applyForEachIdentity(
    nodes: CustomSidebarSwiftNode[],
    identity: string | undefined,
  ): CustomSidebarSwiftNode[] {
    if (identity === undefined) return nodes;
    return nodes.map((node) => ({
      ...node,
      modifiers: [
        ...(node.modifiers ?? []),
        { name: "id", value: identity } satisfies CustomSidebarSwiftModifier,
      ],
    }));
  }

  private forEachIdentityValue(
    expression: string | undefined,
    value: unknown,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const token = expression.trim().replace(/^\\?\./, "");
    if (token === "self") return this.swiftStringValue(value);
    if (typeof value === "object" && value !== null) {
      const resolved = (value as Record<string, unknown>)[token];
      return this.swiftStringValue(resolved);
    }
    return undefined;
  }

  private swiftStringValue(value: unknown): string | undefined {
    if (value === undefined || value === null) return undefined;
    if (typeof value === "string") return value;
    if (typeof value === "number" || typeof value === "boolean") return String(value);
    return undefined;
  }

  private parseForLoop(
    body: string,
    index: number,
    scope: SwiftParseScope,
  ): { nodes: CustomSidebarSwiftNode[]; end: number } {
    const open = body.indexOf("{", index);
    if (open < 0) return { nodes: [], end: index + 3 };
    const header = body.slice(index + 3, open).trim();
    const block = this.readBalanced(body, open, "{", "}");
    if (block === null) return { nodes: [], end: open + 1 };
    const match = header.match(/^([A-Za-z_][A-Za-z0-9_]*)\s+in\s+(.+)$/);
    if (!match) {
      this.warnings.push(`Unsupported for loop '${header}'`);
      return { nodes: [], end: block.end };
    }
    const values = this.evalSequence(match[2] ?? "", scope);
    if (values === null) {
      this.warnings.push(`Unsupported for loop sequence '${match[2] ?? ""}'`);
      return { nodes: [], end: block.end };
    }
    const param = match[1] ?? "item";
    return {
      nodes: values.flatMap((value) =>
        this.parseStatements(block.content, this.scopeWithLoopValue(scope, param, value)),
      ),
      end: block.end,
    };
  }

  private parseIf(
    body: string,
    index: number,
    scope: SwiftParseScope,
  ): { nodes: CustomSidebarSwiftNode[]; end: number } {
    const open = body.indexOf("{", index);
    if (open < 0) return { nodes: [], end: index + 2 };
    const condition = body.slice(index + 2, open).trim();
    const block = this.readBalanced(body, open, "{", "}");
    if (block === null) return { nodes: [], end: open + 1 };
    const afterThen = this.skipWhitespaceAndSeparators(body, block.end);
    let elseBlock: { content: string; end: number } | null = null;
    if (body.startsWith("else", afterThen)) {
      const elseOpen = body.indexOf("{", afterThen + 4);
      elseBlock = elseOpen >= 0 ? this.readBalanced(body, elseOpen, "{", "}") : null;
    }
    const optionalBinding = condition.match(/^let\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.+)$/);
    if (optionalBinding) {
      const value = this.evalOptionalBindingValue(optionalBinding[2] ?? "", scope);
      const passed = value !== undefined && value !== null;
      const bindingScope = passed
        ? ({ ...scope, [optionalBinding[1] ?? "value"]: value } as SwiftParseScope)
        : scope;
      return {
        nodes: this.parseStatements(
          passed ? block.content : (elseBlock?.content ?? ""),
          bindingScope,
        ),
        end: elseBlock?.end ?? block.end,
      };
    }
    const passed = this.evalCondition(condition, scope);
    return {
      nodes: this.parseStatements(
        passed ? block.content : (elseBlock?.content ?? ""),
        scope,
      ),
      end: elseBlock?.end ?? block.end,
    };
  }

  private evalOptionalBindingValue(expression: string, scope: SwiftParseScope): unknown {
    const resolved = this.resolvePath(expression.trim(), scope);
    if (resolved !== undefined) return resolved;
    const evaluated = this.evalExpression(expression, scope);
    return evaluated === expression.trim() ? undefined : evaluated;
  }

  private parseButton(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const actionArg = this.extractNamedClosure(args, "action");
    const role = this.parseButtonRole(args, scope);
    if (actionArg !== null) {
      const localAction = this.parseLocalButtonAction(actionArg);
      return {
        kind: "button",
        role,
        children: this.parseStatements(trailingBody, scope),
        action: this.parseCmuxAction(actionArg, scope),
        ...(localAction !== undefined ? { localAction } : {}),
      };
    }
    const localAction = this.parseLocalButtonAction(trailingBody);
    return {
      kind: "button",
      role,
      text: this.evalExpression(args.split(",")[0] ?? "", scope),
      children: [],
      action: this.parseCmuxAction(trailingBody, scope),
      ...(localAction !== undefined ? { localAction } : {}),
    };
  }

  private parseLocalButtonAction(body: string): "dismissPresentation" | undefined {
    return /\bdismiss\s*\(\s*\)/.test(body) ? "dismissPresentation" : undefined;
  }

  private parseNavigationLink(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const destinationArg = this.namedArg(args, "destination");
    const valueArg = this.namedArg(args, "value");
    const title = this.firstPositionalArg(args, scope) ?? "Open";
    const destination =
      destinationArg === undefined && valueArg === undefined
        ? this.parseStatements(trailingBody, scope)
        : destinationArg === undefined
          ? []
          : this.parseSingleViewArgument(destinationArg, scope);
    const labelChildren =
      destinationArg === undefined && valueArg === undefined
        ? [{ kind: "text", text: title } satisfies CustomSidebarSwiftNode]
        : this.parseStatements(trailingBody, scope);
    return {
      kind: "navigationLink",
      title,
      children:
        labelChildren.length > 0
          ? labelChildren
          : [{ kind: "text", text: title } satisfies CustomSidebarSwiftNode],
      destination,
      ...(valueArg !== undefined ? { value: this.evalExpression(valueArg, scope) } : {}),
    };
  }

  private parseExternalLink(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const destinationArg = this.namedArg(args, "destination");
    const title = this.firstPositionalArg(args, scope);
    const labelChildren = this.parseStatements(trailingBody, scope);
    return {
      kind: "externalLink",
      title,
      href:
        destinationArg === undefined
          ? undefined
          : this.safeExternalLinkUrl(destinationArg, scope),
      labelChildren,
    };
  }

  private parseContentUnavailableView(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
    labeledTrailingClosures: Record<string, string> = {},
  ): CustomSidebarSwiftNode {
    const labelClosure =
      this.extractNamedClosure(args, "label") ??
      labeledTrailingClosures.label ??
      (trailingBody.trim() === "" ? null : trailingBody);
    const descriptionClosure =
      this.extractNamedClosure(args, "description") ??
      labeledTrailingClosures.description ??
      null;
    const actionsClosure =
      this.extractNamedClosure(args, "actions") ??
      labeledTrailingClosures.actions ??
      null;
    const descriptionArg = this.namedArg(args, "description");
    const descriptionChildren =
      descriptionClosure !== null && descriptionClosure.trim() !== ""
        ? this.parseStatements(descriptionClosure, scope)
        : descriptionArg !== undefined
          ? this.parseSingleViewArgument(descriptionArg, scope)
          : [];
    return {
      kind: "contentUnavailable",
      title: this.firstPositionalArg(args, scope),
      systemImage: this.namedStringArg(args, "systemImage", scope),
      labelChildren:
        labelClosure === null || labelClosure.trim() === ""
          ? []
          : this.parseStatements(labelClosure, scope),
      descriptionChildren,
      actionsChildren:
        actionsClosure === null || actionsClosure.trim() === ""
          ? []
          : this.parseStatements(actionsClosure, scope),
    };
  }

  private parseSingleViewArgument(
    expression: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode[] {
    const node = this.parseViewExpression(expression, scope);
    return node === null ? [] : [node];
  }

  private parsePresentationModifier(
    name: "sheet" | "popover" | "fullScreenCover" | "alert" | "confirmationDialog",
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftModifier | null {
    const isPresented = this.namedArg(args, "isPresented");
    const item = this.namedArg(args, "item");
    if (isPresented === undefined && item === undefined) return null;
    const binding = isPresented ?? item ?? "";
    const stateBindingKey = this.stateBindingKey(binding, scope);
    const title = this.firstPositionalArg(args, scope);
    if (item !== undefined) {
      const itemValue = this.swiftStateValue(this.evalBindingValue(item, scope));
      const closure = this.splitClosureParameter(trailingBody);
      const itemParam = closure.params[0] ?? "item";
      return {
        name,
        value: title,
        boolValue: itemValue !== null,
        itemParam,
        itemValue,
        presentationBindingKind: "item",
        ...(stateBindingKey !== undefined ? { stateBindingKey } : {}),
        children: this.parseStatements(
          closure.body,
          this.scopeWithLoopValue(scope, itemParam, itemValue),
        ),
      };
    }
    return {
      name,
      value: title,
      boolValue: this.evalBindingCondition(binding, scope),
      presentationBindingKind: "isPresented",
      ...(stateBindingKey !== undefined ? { stateBindingKey } : {}),
      children: this.parseStatements(trailingBody, scope),
    };
  }

  private parseScrollView(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const first = this.firstPositionalArg(args, scope);
    const axis = this.scrollAxis(first);
    const showsIndicatorsArg = this.namedArg(args, "showsIndicators");
    return {
      kind: "scrollView",
      axis,
      showsIndicators:
        showsIndicatorsArg === undefined ? true : this.evalCondition(showsIndicatorsArg, scope),
      children: this.parseStatements(trailingBody, scope),
    };
  }

  private scrollAxis(value: string | undefined): "vertical" | "horizontal" | "both" {
    const normalized = value?.replace(/^\./, "");
    if (normalized === "horizontal") return "horizontal";
    if (normalized === "vertical") return "vertical";
    if (normalized?.includes("horizontal") && normalized.includes("vertical")) return "both";
    return "vertical";
  }

  private stackSpacing(args: string, scope: SwiftParseScope): number | undefined {
    return this.toNumber(this.namedArg(args, "spacing"), scope);
  }

  private parseButtonRole(
    args: string,
    scope: SwiftParseScope,
  ): "destructive" | "cancel" | undefined {
    const role = this.namedStringArg(args, "role", scope)?.replace(/^\./, "");
    return role === "destructive" || role === "cancel" ? role : undefined;
  }

  private swiftTypeToken(value: string | undefined): string | undefined {
    return value
      ?.trim()
      .replace(/\.self$/, "")
      .replace(/^\./, "")
      .replace(/^[A-Za-z_][A-Za-z0-9_]*\./, "");
  }

  private parseProgressView(
    args: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const valueArg = this.namedArg(args, "value") ?? this.splitTopLevel(args, ",")[0];
    const totalArg = this.namedArg(args, "total");
    const value = this.toNumber(valueArg, scope);
    const total = this.toNumber(totalArg, scope);
    return {
      kind: "progress",
      value,
      total: total ?? (value === undefined ? undefined : 1),
    };
  }

  private parseGauge(
    args: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const valueArg = this.namedArg(args, "value") ?? this.splitTopLevel(args, ",")[0];
    const value = this.toNumber(valueArg, scope);
    const bounds = this.parseRangeBounds(this.namedArg(args, "in"), scope);
    if (value !== undefined && bounds !== undefined) {
      return {
        kind: "progress",
        value: value - bounds.lower,
        total: bounds.upper - bounds.lower,
      };
    }
    return {
      kind: "progress",
      value,
      total: value === undefined ? undefined : 1,
    };
  }

  private parseTextField(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
    options: { secure?: boolean; multiline?: boolean } = {},
  ): CustomSidebarSwiftNode {
    const textBinding = this.namedArg(args, "text") ?? "";
    const stateBindingKey = this.stateBindingKey(textBinding, scope);
    return {
      kind: "textField",
      placeholder: this.firstPositionalArg(args, scope),
      text: this.evalBindingExpression(textBinding, scope),
      ...(options.secure === true ? { secure: true } : {}),
      ...(options.multiline === true ? { multiline: true } : {}),
      ...(stateBindingKey !== undefined ? { stateBindingKey } : {}),
      children: this.parseStatements(trailingBody, scope),
    };
  }

  private parseStepper(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const valueBinding = this.namedArg(args, "value") ?? "";
    const bounds = this.parseRangeBounds(this.namedArg(args, "in"), scope);
    const stateBindingKey = this.stateBindingKey(valueBinding, scope);
    return {
      kind: "stepper",
      title: this.firstPositionalArg(args, scope),
      value: this.toNumber(this.unwrapBindingExpression(valueBinding), scope),
      lowerBound: bounds?.lower ?? Number.NEGATIVE_INFINITY,
      upperBound: bounds?.upper ?? Number.POSITIVE_INFINITY,
      step: this.toNumber(this.namedArg(args, "step"), scope) ?? 1,
      ...(stateBindingKey !== undefined ? { stateBindingKey } : {}),
      children: this.parseStatements(trailingBody, scope),
    };
  }

  private parseSlider(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const bounds = this.parseRangeBounds(this.namedArg(args, "in"), scope);
    const valueBinding = this.namedArg(args, "value") ?? "";
    const stateBindingKey = this.stateBindingKey(valueBinding, scope);
    const value = this.toNumber(
      this.unwrapBindingExpression(valueBinding),
      scope,
    );
    return {
      kind: "slider",
      value,
      lowerBound: bounds?.lower ?? 0,
      upperBound: bounds?.upper ?? 1,
      ...(stateBindingKey !== undefined ? { stateBindingKey } : {}),
      children: this.parseStatements(trailingBody, scope),
    };
  }

  private parsePicker(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const selectionBinding = this.namedArg(args, "selection") ?? "";
    const stateBindingKey = this.stateBindingKey(selectionBinding, scope);
    const selectedValue = this.swiftStateValue(this.evalBindingValue(selectionBinding, scope));
    const children = this.parseStatements(trailingBody, scope);
    const options = this.swiftPickerOptions(children);
    return {
      kind: "picker",
      title: this.firstPositionalArg(args, scope),
      selection: this.swiftPickerSelectionLabel(selectedValue, options),
      selectedValue,
      options,
      ...(stateBindingKey !== undefined ? { stateBindingKey } : {}),
      children,
    };
  }

  private swiftPickerOptions(nodes: CustomSidebarSwiftNode[]): CustomSidebarSwiftPickerOption[] {
    const options = nodes.flatMap((node) => this.swiftPickerOptionsFromNode(node));
    const unique = new Map<string, CustomSidebarSwiftPickerOption>();
    for (const option of options) {
      if (!unique.has(option.encodedValue)) unique.set(option.encodedValue, option);
    }
    return [...unique.values()];
  }

  private swiftPickerOptionsFromNode(node: CustomSidebarSwiftNode): CustomSidebarSwiftPickerOption[] {
    const tagValue = this.swiftNodeTagValue(node);
    const label = this.swiftPickerNodeLabel(node);
    if (label !== undefined) {
      const value = tagValue ?? label;
      return [
        {
          label,
          value,
          encodedValue: this.swiftPickerEncodedValue(value),
        },
      ];
    }
    if (node.kind === "modified") return this.swiftPickerOptionsFromNode(node.base);
    if (
      node.kind === "group" ||
      node.kind === "vstack" ||
      node.kind === "hstack" ||
      node.kind === "gridRow"
    ) {
      return node.children.flatMap((child) => this.swiftPickerOptionsFromNode(child));
    }
    return [];
  }

  private swiftPickerNodeLabel(node: CustomSidebarSwiftNode): string | undefined {
    if (node.kind === "text" && node.text.trim() !== "") return node.text;
    if (node.kind === "label" && node.text.trim() !== "") return node.text;
    if (node.kind === "modified") return this.swiftPickerNodeLabel(node.base);
    if (
      node.kind === "group" ||
      node.kind === "vstack" ||
      node.kind === "hstack" ||
      node.kind === "gridRow"
    ) {
      return node.children
        .map((child) => this.swiftPickerNodeLabel(child))
        .find((childLabel) => childLabel !== undefined);
    }
    return undefined;
  }

  private swiftNodeTagValue(node: CustomSidebarSwiftNode): SwiftSidebarStateValue | undefined {
    const tag = node.modifiers?.find((modifier) => modifier.name === "tag");
    if (tag?.tagValue !== undefined) return tag.tagValue;
    if (node.kind === "modified") return this.swiftNodeTagValue(node.base);
    return undefined;
  }

  private swiftPickerSelectionLabel(
    selectedValue: SwiftSidebarStateValue,
    options: CustomSidebarSwiftPickerOption[],
  ): string {
    const selected = this.swiftPickerEncodedValue(selectedValue);
    return options.find((option) => option.encodedValue === selected)?.label ?? String(selectedValue ?? "");
  }

  private swiftPickerEncodedValue(value: SwiftSidebarStateValue): string {
    return JSON.stringify(value);
  }

  private parseDatePicker(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const selectionBinding = this.namedArg(args, "selection") ?? "";
    const stateBindingKey = this.stateBindingKey(selectionBinding, scope);
    const displayedComponents = this.namedArg(args, "displayedComponents");
    return {
      kind: "datePicker",
      title: this.firstPositionalArg(args, scope),
      value: this.evalBindingExpression(selectionBinding, scope),
      ...(displayedComponents !== undefined
        ? { displayedComponents: this.evalExpression(displayedComponents, scope) }
        : {}),
      ...(stateBindingKey !== undefined ? { stateBindingKey } : {}),
      children: this.parseStatements(trailingBody, scope),
    };
  }

  private parseColorPicker(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const selectionBinding = this.namedArg(args, "selection") ?? "";
    const stateBindingKey = this.stateBindingKey(selectionBinding, scope);
    return {
      kind: "colorPicker",
      title: this.firstPositionalArg(args, scope),
      value: this.evalBindingExpression(selectionBinding, scope),
      ...(stateBindingKey !== undefined ? { stateBindingKey } : {}),
      children: this.parseStatements(trailingBody, scope),
    };
  }

  private parseList(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const collection = this.firstPositionalExpression(args);
    if (collection === undefined) {
      return {
        kind: "list",
        children: this.parseStatements(trailingBody, scope),
      };
    }
    const values = this.evalSequence(collection, scope);
    if (values === null) {
      this.warnings.push(`Unsupported List collection '${collection}'`);
      return {
        kind: "list",
        children: this.parseStatements(trailingBody, scope),
      };
    }
    const closure = this.splitClosureParameter(trailingBody);
    const params = closure.params.length > 0 ? closure.params : ["item"];
    return {
      kind: "list",
      dataId: this.swiftListIdToken(this.namedArg(args, "id"), scope),
      children: values.flatMap((value) =>
        this.parseStatements(closure.body, this.scopeWithLoopValues(scope, params, value)),
      ),
    };
  }

  private parseSection(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const headerExpression = this.namedArg(args, "header");
    const footerExpression = this.namedArg(args, "footer");
    return {
      kind: "section",
      title: this.firstPositionalArg(args, scope),
      ...(headerExpression !== undefined
        ? { header: this.parseViewExpressionList(headerExpression, scope) }
        : {}),
      ...(footerExpression !== undefined
        ? { footer: this.parseViewExpressionList(footerExpression, scope) }
        : {}),
      children: this.parseStatements(trailingBody, scope),
    };
  }

  private parseGrid(
    name: "Grid" | "LazyVGrid" | "LazyHGrid",
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const gridKind =
      name === "LazyVGrid" ? "lazyVGrid" : name === "LazyHGrid" ? "lazyHGrid" : "grid";
    const itemExpression =
      name === "LazyHGrid"
        ? this.namedArg(args, "rows") ?? this.firstPositionalExpression(args)
        : name === "LazyVGrid"
          ? this.namedArg(args, "columns") ?? this.firstPositionalExpression(args)
          : undefined;
    return {
      kind: "grid",
      gridKind,
      ...(itemExpression !== undefined
        ? { gridItems: this.swiftGridItems(itemExpression, scope) }
        : {}),
      spacing: this.toNumber(this.namedArg(args, "spacing"), scope),
      alignment: this.swiftAlignmentToken(this.namedArg(args, "alignment"), scope),
      ...(gridKind !== "grid"
        ? { pinnedViews: this.swiftPinnedViews(this.namedArg(args, "pinnedViews"), scope) }
        : {}),
      children: this.parseStatements(trailingBody, scope),
    };
  }

  private parseViewExpressionList(
    expression: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode[] {
    const trimmed = expression.trim();
    if (trimmed === "") return [];
    const node = this.parseViewExpression(trimmed, scope);
    return node === null ? [] : [node];
  }

  private parseToggle(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const isOnBinding = this.namedArg(args, "isOn") ?? "false";
    const stateBindingKey = this.stateBindingKey(isOnBinding, scope);
    return {
      kind: "toggle",
      text: this.firstPositionalArg(args, scope),
      isOn: this.evalBindingCondition(isOnBinding, scope),
      ...(stateBindingKey !== undefined ? { stateBindingKey } : {}),
      children: this.parseStatements(trailingBody, scope),
    };
  }

  private withModifiers(
    node: CustomSidebarSwiftNode,
    text: string,
    modifierStart: number,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const modifiers = this.parseModifiers(text, modifierStart, scope);
    if (modifiers.length === 0) return node;
    const nodeWithRouteDestinations =
      node.kind === "navigationStack"
        ? this.resolveNavigationValueDestinations(node, modifiers, scope)
        : node;
    const tapModifier = modifiers.find((modifier) => modifier.name === "onTapGesture");
    const longPressModifier = modifiers.find(
      (modifier) => modifier.name === "onLongPressGesture",
    );
    const eventHandlers = modifiers
      .map((modifier) => modifier.eventHandler)
      .filter((handler): handler is CustomSidebarSwiftEventHandler => handler !== undefined);
    const childModifiers = modifiers.filter((modifier) =>
      this.isChildModifier(modifier),
    );
    const hasNavigationChrome = modifiers.some((modifier) =>
      this.isNavigationModifier(modifier),
    );
    const visualModifiers = modifiers.filter(
      (modifier) =>
        modifier.name !== "onTapGesture" &&
        modifier.name !== "onLongPressGesture" &&
        modifier.name !== "onEvent" &&
        !this.isChildModifier(modifier),
    );
    const decorated =
      visualModifiers.length === 0 && eventHandlers.length === 0
        ? nodeWithRouteDestinations
        : {
            ...nodeWithRouteDestinations,
            ...(visualModifiers.length > 0 ? { modifiers: visualModifiers } : {}),
            ...(eventHandlers.length > 0
              ? { eventHandlers: [...(nodeWithRouteDestinations.eventHandlers ?? []), ...eventHandlers] }
              : {}),
          };
    const gestureAction = tapModifier?.action ?? longPressModifier?.action;
    const actionTrigger = longPressModifier?.action !== undefined ? "longPress" : "tap";
    const tappable =
      gestureAction === undefined
        ? decorated
        : {
            kind: "button",
            children: [decorated],
            action: gestureAction,
            actionTrigger,
            tapCount: tapModifier?.count,
            modifiers: [{ name: "buttonStyle", value: "plain" }],
          } satisfies CustomSidebarSwiftNode;
    if (childModifiers.length === 0 && !hasNavigationChrome) return tappable;
    return {
      kind: "modified",
      base: tappable,
      childModifiers,
    };
  }

  private resolveNavigationValueDestinations(
    node: Extract<CustomSidebarSwiftNode, { kind: "navigationStack" }>,
    modifiers: CustomSidebarSwiftModifier[],
    scope: SwiftParseScope,
  ): Extract<CustomSidebarSwiftNode, { kind: "navigationStack" }> {
    const routeDestinations = modifiers.filter(
      (modifier) => modifier.name === "navigationDestination" && modifier.routeBody !== undefined,
    );
    if (routeDestinations.length === 0) return node;
    return {
      ...node,
      children: node.children.map((child) =>
        this.resolveNavigationValueDestinationNode(child, routeDestinations, scope),
      ),
    };
  }

  private resolveNavigationValueDestinationNode(
    node: CustomSidebarSwiftNode,
    routeDestinations: CustomSidebarSwiftModifier[],
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    if (node.kind === "navigationLink" && node.value !== undefined && node.destination.length === 0) {
      const destination = routeDestinations[0];
      const routeParam = destination?.routeParam ?? "value";
      const routeBody = destination?.routeBody ?? "";
      return {
        ...node,
        destination: this.parseStatements(
          routeBody,
          this.scopeWithLoopValue(scope, routeParam, node.value),
        ),
      };
    }
    if (
      node.kind === "vstack" ||
      node.kind === "hstack" ||
      node.kind === "zstack" ||
      node.kind === "group" ||
      node.kind === "splitView" ||
      node.kind === "tabView" ||
      node.kind === "scrollView" ||
      node.kind === "list" ||
      node.kind === "section" ||
      node.kind === "groupBox" ||
      node.kind === "disclosureGroup" ||
      node.kind === "grid" ||
      node.kind === "gridRow" ||
      node.kind === "menu" ||
      node.kind === "button"
    ) {
      return {
        ...node,
        children: node.children.map((child) =>
          this.resolveNavigationValueDestinationNode(child, routeDestinations, scope),
        ),
        ...(node.kind === "groupBox" || node.kind === "disclosureGroup"
          ? {
              labelChildren: node.labelChildren.map((child) =>
                this.resolveNavigationValueDestinationNode(child, routeDestinations, scope),
              ),
            }
          : {}),
      };
    }
    if (node.kind === "externalLink") {
      return {
        ...node,
        labelChildren: node.labelChildren.map((child) =>
          this.resolveNavigationValueDestinationNode(child, routeDestinations, scope),
        ),
      };
    }
    if (node.kind === "contentUnavailable") {
      return {
        ...node,
        labelChildren: node.labelChildren.map((child) =>
          this.resolveNavigationValueDestinationNode(child, routeDestinations, scope),
        ),
        descriptionChildren: node.descriptionChildren.map((child) =>
          this.resolveNavigationValueDestinationNode(child, routeDestinations, scope),
        ),
        actionsChildren: node.actionsChildren.map((child) =>
          this.resolveNavigationValueDestinationNode(child, routeDestinations, scope),
        ),
      };
    }
    return node;
  }

  private isNavigationModifier(modifier: CustomSidebarSwiftModifier): boolean {
    return (
      modifier.name === "navigationTitle" ||
      modifier.name === "navigationSubtitle" ||
      modifier.name === "navigationBarTitleDisplayMode"
    );
  }

  private isChildModifier(modifier: CustomSidebarSwiftModifier): boolean {
    return (
      (modifier.name === "background" ||
        modifier.name === "overlay" ||
        modifier.name === "mask" ||
        modifier.name === "safeAreaInset" ||
        modifier.name === "contextMenu" ||
        modifier.name === "refreshable" ||
        modifier.name === "swipeActions" ||
        modifier.name === "sheet" ||
        modifier.name === "popover" ||
        modifier.name === "fullScreenCover" ||
        modifier.name === "alert" ||
        modifier.name === "confirmationDialog" ||
        modifier.name === "toolbar" ||
        modifier.name === "searchable" ||
        modifier.name === "accessibilityRepresentation" ||
        modifier.name === "tabItem") &&
      ((modifier.children?.length ?? 0) > 0 ||
        modifier.action !== undefined ||
        modifier.name === "searchable" ||
        modifier.name === "refreshable" ||
        modifier.name === "alert" ||
        modifier.name === "confirmationDialog")
    );
  }

  private parseModifiers(
    text: string,
    start: number,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftModifier[] {
    const modifiers: CustomSidebarSwiftModifier[] = [];
    let index = start;
    while (index < text.length && modifiers.length < 40) {
      index = this.skipWhitespaceAndSeparators(text, index);
      if (text[index] !== ".") break;
      const call = this.readModifierCall(text.slice(index + 1));
      if (call === null) break;
      index += 1 + call.end;
      const trailing = this.readTrailingClosure(text.slice(index));
      const modifier = this.parseModifier(
        call.name,
        call.args,
        scope,
        trailing?.body,
      );
      if (modifier !== null) modifiers.push(modifier);
      if (trailing !== null) index += trailing.end;
    }
    return modifiers;
  }

  private parseModifier(
    name: string,
    args: string,
    scope: SwiftParseScope,
    trailingBody?: string,
  ): CustomSidebarSwiftModifier | null {
    switch (name) {
      case "onTapGesture": {
        const action = this.parseCmuxAction(trailingBody ?? args, scope);
        return action === undefined
          ? null
          : {
              name: "onTapGesture",
              action,
              count: Math.max(1, Math.floor(this.toNumber(this.namedArg(args, "count"), scope) ?? 1)),
            };
      }
      case "onLongPressGesture": {
        const action = this.parseCmuxAction(trailingBody ?? args, scope);
        return action === undefined ? null : { name: "onLongPressGesture", action };
      }
      case "onHover": {
        const closure = this.splitClosureParameter(trailingBody ?? "");
        const hoverParam = closure.params[0] ?? "hovering";
        const enterHandler = this.parseLocalHandler(
          closure.body,
          this.scopeWithLoopValue(scope, hoverParam, true),
          "onHover",
        );
        const leaveHandler = this.parseLocalHandler(
          closure.body,
          this.scopeWithLoopValue(scope, hoverParam, false),
          "onHover",
        );
        return enterHandler === null && leaveHandler === null
          ? null
          : {
              name: "onHover",
              value: hoverParam,
              ...(enterHandler !== null ? { localHandler: enterHandler } : {}),
              ...(leaveHandler !== null ? { falseLocalHandler: leaveHandler } : {}),
            };
      }
      case "onEvent": {
        const eventHandler = this.parseEventHandler(args, trailingBody ?? "", scope);
        return eventHandler === null ? null : { name: "onEvent", eventHandler };
      }
      case "onSubmit": {
        const localHandler = this.parseLocalHandler(trailingBody ?? args, scope, name);
        return localHandler === null ? null : { name: "onSubmit", localHandler };
      }
      case "onChange": {
        const localHandler = this.parseLocalHandler(trailingBody ?? "", scope, name);
        if (localHandler === null) return null;
        const observedExpression =
          this.namedArg(args, "of") ?? this.splitTopLevel(args, ",")[0]?.trim();
        return {
          name: "onChange",
          localHandler,
          ...(observedExpression !== undefined && observedExpression !== ""
            ? {
                value: this.evalExpression(observedExpression, scope),
                secondaryValue: observedExpression,
              }
            : {}),
        };
      }
      case "onGeometryChange": {
        return {
          name: "onGeometryChange",
          value: this.swiftTypeToken(this.namedArg(args, "for") ?? this.splitTopLevel(args, ",")[0]),
          boolValue: trailingBody !== undefined && trailingBody.trim() !== "",
        };
      }
      case "onAppear":
      case "onDisappear": {
        const localHandler = this.parseLocalHandler(trailingBody ?? args, scope, name);
        return localHandler === null ? null : { name, localHandler };
      }
      case "task": {
        const localHandler = this.parseLocalHandler(trailingBody ?? "", scope, "task");
        if (localHandler === null) return null;
        const taskIdExpression = this.namedArg(args, "id");
        return {
          name: "task",
          localHandler,
          ...(taskIdExpression !== undefined && taskIdExpression !== ""
            ? {
                value: this.evalExpression(taskIdExpression, scope),
                secondaryValue: taskIdExpression,
              }
            : {}),
        };
      }
      case "tag": {
        const tagArg = this.splitTopLevel(args, ",")[0] ?? "";
        const tagValue = this.swiftStateValue(this.evalValue(tagArg, scope));
        return {
          name: "tag",
          value: String(tagValue ?? ""),
          tagValue,
        };
      }
      case "tabItem":
        return {
          name: "tabItem",
          children: this.parseStatements(trailingBody ?? "", scope),
        };
      case "id":
        return { name: "id", value: this.evalExpression(this.splitTopLevel(args, ",")[0] ?? "", scope) };
      case "animation":
        return {
          name: "animation",
          value: this.firstPositionalArg(args, scope),
          secondaryValue: this.namedArg(args, "value"),
        };
      case "transition":
        return { name: "transition", value: this.firstPositionalArg(args, scope) };
      case "contentTransition":
        return { name: "contentTransition", value: this.firstPositionalArg(args, scope) };
      case "symbolEffect":
        return {
          name: "symbolEffect",
          value: this.firstPositionalArg(args, scope),
          boolValue:
            this.namedArg(args, "isActive") === undefined
              ? undefined
              : this.evalCondition(this.namedArg(args, "isActive") ?? "false", scope),
          secondaryValue: this.namedArg(args, "value"),
        };
      case "symbolEffectsRemoved":
        return {
          name: "symbolEffectsRemoved",
          boolValue:
            args.trim() === ""
              ? true
              : this.evalCondition(this.splitTopLevel(args, ",")[0] ?? "true", scope),
        };
      case "buttonStyle":
        return { name: "buttonStyle", value: this.firstPositionalArg(args, scope) };
      case "font":
        return { name: "font", value: this.firstPositionalArg(args, scope) };
      case "fontWeight":
        return { name: "fontWeight", value: this.firstPositionalArg(args, scope) };
      case "fontDesign":
        return { name: "fontDesign", value: this.firstPositionalArg(args, scope) };
      case "fontWidth":
        return { name: "fontWidth", value: this.firstPositionalArg(args, scope) };
      case "dynamicTypeSize":
        return { name: "dynamicTypeSize", value: this.firstPositionalArg(args, scope) };
      case "bold":
        return args.trim() === ""
          ? { name: "bold" }
          : {
              name: "bold",
              boolValue: this.evalCondition(this.firstPositionalExpression(args) ?? "true", scope),
            };
      case "italic":
        return args.trim() === ""
          ? { name: "italic" }
          : {
              name: "italic",
              boolValue: this.evalCondition(this.firstPositionalExpression(args) ?? "true", scope),
            };
      case "monospaced":
        return { name: "monospaced" };
      case "monospacedDigit":
        return { name: "monospacedDigit" };
      case "foregroundColor":
      case "foregroundStyle":
      case "fill":
      case "tint":
        return {
          name: "foregroundColor",
          value: this.firstPositionalArg(args, scope),
        };
      case "padding": {
        const parts = this.splitTopLevel(args, ",");
        const namedEdges = this.namedArg(args, "edges");
        const namedLength = this.namedArg(args, "length");
        const first = parts[0];
        const insetPadding = this.swiftPaddingInsetsValue(
          namedEdges === undefined && namedLength === undefined ? first : undefined,
          scope,
        );
        if (Object.keys(insetPadding).length > 0) {
          return { name: "padding", ...insetPadding };
        }
        const firstIsEdgeSet = this.swiftEdgeSet(namedEdges ?? first, scope) !== undefined;
        const edge = this.swiftEdgeSet(namedEdges ?? (firstIsEdgeSet ? first : undefined), scope);
        const amountExpression =
          namedLength ?? (edge === undefined ? parts.at(-1) : (parts[1] ?? undefined));
        const amount = this.toNumber(amountExpression, scope)?.toString();
        return { name: "padding", value: amount, edge: edge?.join(",") };
      }
      case "safeAreaPadding":
      case "contentMargins": {
        const parts = this.splitTopLevel(args, ",");
        const namedEdges = this.namedArg(args, "edges");
        const namedLength = this.namedArg(args, "length");
        const first = parts[0];
        const insetPadding = this.swiftPaddingInsetsValue(
          namedEdges === undefined && namedLength === undefined ? first : undefined,
          scope,
        );
        const placement = this.swiftContentMarginPlacementToken(this.namedArg(args, "for"), scope);
        if (Object.keys(insetPadding).length > 0) {
          return {
            name,
            ...insetPadding,
            ...(placement !== undefined ? { placement } : {}),
          };
        }
        const firstIsEdgeSet = this.swiftEdgeSet(namedEdges ?? first, scope) !== undefined;
        const edge = this.swiftEdgeSet(namedEdges ?? (firstIsEdgeSet ? first : undefined), scope);
        const amountExpression =
          namedLength ?? (edge === undefined ? first : (parts[1] ?? undefined));
        const amount = this.toNumber(amountExpression, scope)?.toString();
        return {
          name,
          value: amount,
          edge: edge?.join(","),
          ...(placement !== undefined ? { placement } : {}),
        };
      }
      case "gridCellColumns":
        return {
          name: "gridCellColumns",
          value: this.toNumber(this.firstPositionalExpression(args), scope)?.toString(),
        };
      case "gridColumnAlignment":
        return { name: "gridColumnAlignment", value: this.firstPositionalArg(args, scope) };
      case "gridCellAnchor":
        return {
          name: "gridCellAnchor",
          value:
            this.swiftUnitPointToken(this.firstPositionalExpression(args), scope) ??
            this.firstPositionalArg(args, scope),
        };
      case "background":
        if (trailingBody !== undefined) {
          return {
            name: "background",
            children: this.parseStatements(trailingBody, scope),
          };
        }
        return { name: "background", value: this.firstPositionalArg(args, scope) };
      case "overlay":
        return {
          name: "overlay",
          value: this.namedArg(args, "alignment")?.replace(/^\./, ""),
          children: this.parseStatements(trailingBody ?? "", scope),
        };
      case "mask":
        return {
          name: "mask",
          children: this.parseStatements(trailingBody ?? "", scope),
        };
      case "safeAreaInset":
        return {
          name: "safeAreaInset",
          edge: this.namedArg(args, "edge")?.replace(/^\./, ""),
          children: this.parseStatements(trailingBody ?? "", scope),
        };
      case "contextMenu":
        return {
          name: "contextMenu",
          children: this.parseStatements(trailingBody ?? "", scope),
        };
      case "refreshable": {
        const action = this.parseCmuxAction(trailingBody ?? "", scope);
        return {
          name: "refreshable",
          ...(action !== undefined ? { action } : {}),
        };
      }
      case "swipeActions":
        return {
          name: "swipeActions",
          value: this.namedArg(args, "edge")?.replace(/^\./, ""),
          boolValue:
            this.namedArg(args, "allowsFullSwipe") === undefined
              ? undefined
              : this.evalCondition(this.namedArg(args, "allowsFullSwipe") ?? "true", scope),
          children: this.parseStatements(trailingBody ?? "", scope),
        };
      case "sheet":
      case "popover":
      case "fullScreenCover":
      case "alert":
      case "confirmationDialog":
        return this.parsePresentationModifier(name, args, trailingBody ?? "", scope);
      case "presentationDetents":
        return { name: "presentationDetents", value: this.presentationDetentsValue(args, scope) };
      case "presentationDragIndicator":
        return {
          name: "presentationDragIndicator",
          value: this.firstPositionalArg(args, scope),
        };
      case "presentationBackground":
        return { name: "presentationBackground", value: this.firstPositionalArg(args, scope) };
      case "presentationCornerRadius":
        return {
          name: "presentationCornerRadius",
          value: this.toNumber(this.firstPositionalArg(args, scope), scope)?.toString(),
        };
      case "toolbar":
        return {
          name: "toolbar",
          children: this.parseStatements(trailingBody ?? "", scope),
        };
      case "toolbarBackground":
        return {
          name: "toolbarBackground",
          value: this.firstPositionalArg(args, scope),
          secondaryValue: this.namedArg(args, "for")?.replace(/^\./, ""),
        };
      case "toolbarColorScheme":
        return {
          name: "toolbarColorScheme",
          value: this.firstPositionalArg(args, scope),
          secondaryValue: this.namedArg(args, "for")?.replace(/^\./, ""),
        };
      case "searchable": {
        const binding = this.namedArg(args, "text") ?? this.firstPositionalExpression(args) ?? "";
        return {
          name: "searchable",
          value: this.evalBindingExpression(binding, scope),
          secondaryValue: this.namedArgValue(args, "prompt", scope),
          placement: this.swiftTypeToken(this.namedArg(args, "placement")),
          stateBindingKey: this.stateBindingKey(binding, scope),
        };
      }
      case "navigationTitle":
        return { name: "navigationTitle", value: this.firstPositionalArg(args, scope) };
      case "navigationSubtitle":
        return { name: "navigationSubtitle", value: this.firstPositionalArg(args, scope) };
      case "navigationBarTitleDisplayMode":
        return {
          name: "navigationBarTitleDisplayMode",
          value: this.firstPositionalArg(args, scope),
        };
      case "navigationDestination": {
        const closure = this.splitClosureParameter(trailingBody ?? "");
        return {
          name: "navigationDestination",
          routeParam: closure.params[0] ?? "value",
          routeBody: closure.body,
          routeValueType: this.swiftTypeToken(
            this.namedArg(args, "for") ?? this.splitTopLevel(args, ",")[0],
          ),
        };
      }
      case "keyboardShortcut":
        return {
          name: "keyboardShortcut",
          value: this.firstPositionalArg(args, scope),
          secondaryValue: this.swiftKeyboardShortcutModifiers(this.namedArg(args, "modifiers"), scope),
        };
      case "contentShape":
        return { name: "contentShape", value: this.swiftShapeToken(args) };
      case "coordinateSpace":
        return {
          name: "coordinateSpace",
          value: this.swiftCoordinateSpaceToken(
            this.namedArg(args, "name") ?? this.firstPositionalExpression(args),
            scope,
          ),
        };
      case "draggable":
        return { name: "draggable", value: this.firstPositionalArg(args, scope) };
      case "dropDestination": {
        const action = this.parseCmuxAction(trailingBody ?? "", scope);
        return action === undefined
          ? null
          : {
              name: "dropDestination",
              value: this.swiftTypeToken(
                this.namedArg(args, "for") ?? this.firstPositionalArg(args, scope),
              ),
              action,
            };
      }
      case "focusable": {
        const focusableArg = this.splitTopLevel(args, ",")[0]?.trim();
        return {
          name: "focusable",
          boolValue: focusableArg === undefined || focusableArg === "" ? true : this.evalCondition(focusableArg, scope),
        };
      }
      case "focused": {
        const bindingArg = this.splitTopLevel(args, ",")[0]?.trim() ?? "";
        const stateBindingKey = this.stateBindingKey(bindingArg, scope);
        return {
          name: "focused",
          value: bindingArg,
          ...(stateBindingKey !== undefined ? { stateBindingKey } : {}),
          boolValue: this.evalBindingCondition(bindingArg, scope),
        };
      }
      case "controlSize":
        return { name: "controlSize", value: this.firstPositionalArg(args, scope) };
      case "buttonBorderShape":
        return { name: "buttonBorderShape", value: this.firstPositionalArg(args, scope) };
      case "cornerRadius":
        return {
          name: "cornerRadius",
          value: this.toNumber(this.firstPositionalArg(args, scope), scope)?.toString(),
        };
      case "containerRelativeFrame": {
        const axis = this.scrollAxis(this.firstPositionalArg(args, scope));
        return {
          name: "containerRelativeFrame",
          value: axis,
          count: this.toNumber(this.namedArg(args, "count"), scope),
          span: this.toNumber(this.namedArg(args, "span"), scope),
          spacing: this.toNumber(this.namedArg(args, "spacing"), scope),
          frameAlignment: this.swiftAlignmentToken(this.namedArg(args, "alignment"), scope),
        };
      }
      case "frame":
        return {
          name: "frame",
          frameWidth: this.toNumber(this.namedArg(args, "width"), scope),
          frameHeight: this.toNumber(this.namedArg(args, "height"), scope),
          frameMinWidth: this.toNumber(this.namedArg(args, "minWidth"), scope),
          frameMinHeight: this.toNumber(this.namedArg(args, "minHeight"), scope),
          frameMaxWidth: this.toNumber(this.namedArg(args, "maxWidth"), scope),
          frameMaxHeight: this.toNumber(this.namedArg(args, "maxHeight"), scope),
          frameIdealWidth: this.toNumber(this.namedArg(args, "idealWidth"), scope),
          frameIdealHeight: this.toNumber(this.namedArg(args, "idealHeight"), scope),
          frameAlignment: this.swiftAlignmentToken(this.namedArg(args, "alignment"), scope),
          maxWidthInfinity: this.namedArg(args, "maxWidth") === ".infinity",
        };
      case "alignmentGuide":
        return {
          name: "alignmentGuide",
          value: this.swiftAlignmentGuideToken(this.firstPositionalExpression(args), scope),
          secondaryValue: this.alignmentGuideOffsetValue(trailingBody ?? "", scope),
        };
      case "layoutPriority":
        return {
          name: "layoutPriority",
          value: this.toNumber(this.firstPositionalArg(args, scope), scope)?.toString(),
        };
      case "offset":
        return {
          name: "offset",
          x: this.toNumber(this.namedArg(args, "x"), scope) ?? 0,
          y: this.toNumber(this.namedArg(args, "y"), scope) ?? 0,
        };
      case "position":
        return {
          name: "position",
          x: this.toNumber(this.namedArg(args, "x"), scope) ?? 0,
          y: this.toNumber(this.namedArg(args, "y"), scope) ?? 0,
        };
      case "zIndex":
        return {
          name: "zIndex",
          value: this.toNumber(this.firstPositionalArg(args, scope), scope)?.toString(),
        };
      case "aspectRatio":
        return {
          name: "aspectRatio",
          value: this.firstPositionalArg(args, scope),
          secondaryValue: this.namedArg(args, "contentMode")?.replace(/^\./, ""),
        };
      case "scaledToFit":
        return { name: "aspectRatio", secondaryValue: "fit" };
      case "scaledToFill":
        return { name: "aspectRatio", secondaryValue: "fill" };
      case "clipped":
        return { name: "clipped" };
      case "compositingGroup":
        return { name: "compositingGroup" };
      case "clipShape":
        return {
          name: "clipShape",
          value: this.swiftShapeToken(args),
          ...this.swiftFillStyleMetadata(this.namedArg(args, "style"), scope),
        };
      case "shadow":
        return {
          name: "shadow",
          value: this.namedStringArg(args, "color", scope),
          radius: this.toNumber(this.namedArg(args, "radius"), scope) ?? 8,
          x: this.toNumber(this.namedArg(args, "x"), scope) ?? 0,
          y: this.toNumber(this.namedArg(args, "y"), scope) ?? 3,
        };
      case "border":
      case "stroke":
        return {
          name: "border",
          value: this.firstPositionalArg(args, scope),
          width: this.toNumber(this.namedArg(args, "width"), scope) ?? 1,
        };
      case "strokeBorder":
        return {
          name: "strokeBorder",
          value: this.firstPositionalArg(args, scope),
          width:
            this.toNumber(this.namedArg(args, "lineWidth"), scope) ??
            this.toNumber(this.namedArg(args, "width"), scope) ??
            1,
        };
      case "trim":
        return {
          name: "trim",
          x: this.toNumber(this.namedArg(args, "from"), scope) ?? 0,
          y: this.toNumber(this.namedArg(args, "to"), scope) ?? 1,
        };
      case "blur":
        return {
          name: "blur",
          radius: this.toNumber(this.namedArg(args, "radius"), scope) ?? 0,
        };
      case "brightness":
      case "contrast":
      case "saturation":
      case "grayscale":
        return {
          name,
          value: this.toNumber(this.firstPositionalArg(args, scope), scope)?.toString(),
        };
      case "hueRotation":
        return { name: "hueRotation", value: this.swiftDegrees(args, scope)?.toString() };
      case "blendMode":
        return { name: "blendMode", value: this.firstPositionalArg(args, scope) };
      case "rotationEffect":
        return { name: "rotationEffect", value: this.swiftDegrees(args, scope)?.toString() };
      case "scaleEffect":
        return {
          name: "scaleEffect",
          value: this.toNumber(this.firstPositionalArg(args, scope), scope)?.toString(),
        };
      case "rotation3DEffect": {
        const axis = this.swiftAxisTuple(this.namedArg(args, "axis"), scope);
        return {
          name: "rotation3DEffect",
          value: this.swiftDegrees(args, scope)?.toString(),
          secondaryValue: this.swiftUnitPointToken(this.namedArg(args, "anchor"), scope),
          x: axis?.x ?? 0,
          y: axis?.y ?? 0,
          z: axis?.z ?? 0,
          perspective: this.toNumber(this.namedArg(args, "perspective"), scope),
        };
      }
      case "visualEffect":
        return {
          name: "visualEffect",
          boolValue: trailingBody !== undefined && trailingBody.trim() !== "",
        };
      case "lineLimit":
        return {
          name: "lineLimit",
          value: this.toNumber(this.firstPositionalArg(args, scope), scope)?.toString(),
          boolValue:
            this.namedArg(args, "reservesSpace") === undefined
              ? undefined
              : this.evalCondition(this.namedArg(args, "reservesSpace") ?? "false", scope),
        };
      case "truncationMode":
        return { name: "truncationMode", value: this.firstPositionalArg(args, scope) };
      case "multilineTextAlignment":
        return {
          name: "multilineTextAlignment",
          value: this.firstPositionalArg(args, scope),
        };
      case "textCase":
        return { name: "textCase", value: this.firstPositionalArg(args, scope) };
      case "tracking":
      case "kerning":
      case "baselineOffset":
        return {
          name,
          value: this.toNumber(this.firstPositionalArg(args, scope), scope)?.toString(),
        };
      case "underline":
      case "strikethrough":
        return this.parseTextDecorationModifier(name, args, scope);
      case "opacity":
        return {
          name: "opacity",
          value: this.toNumber(this.firstPositionalArg(args, scope), scope)?.toString(),
        };
      case "hidden":
        return { name: "hidden", boolValue: true };
      case "fixedSize":
        return { name: "fixedSize" };
      case "badge":
        return { name: "badge", value: this.firstPositionalArg(args, scope) };
      case "allowsHitTesting":
        return {
          name: "allowsHitTesting",
          boolValue:
            args.trim() === ""
              ? true
              : this.evalCondition(this.firstPositionalExpression(args) ?? "true", scope),
        };
      case "disabled":
        return { name: "disabled", boolValue: this.evalCondition(args || "true", scope) };
      case "hoverEffect": {
        const isEnabled = this.namedArg(args, "isEnabled");
        return {
          name: "hoverEffect",
          value: this.firstPositionalArg(args, scope) ?? "automatic",
          boolValue: isEnabled === undefined ? true : this.evalCondition(isEnabled, scope),
        };
      }
      case "defaultHoverEffect":
        return {
          name: "defaultHoverEffect",
          value: this.firstPositionalArg(args, scope) ?? "automatic",
        };
      case "help":
        return { name: "help", value: this.firstPositionalArg(args, scope) };
      case "accessibilityLabel":
        return { name: "accessibilityLabel", value: this.firstPositionalArg(args, scope) };
      case "accessibilityHidden":
        return {
          name: "accessibilityHidden",
          boolValue:
            args.trim() === ""
              ? true
              : this.evalCondition(this.splitTopLevel(args, ",")[0] ?? args, scope),
        };
      case "accessibilityValue":
        return { name: "accessibilityValue", value: this.firstPositionalArg(args, scope) };
      case "accessibilityHint":
        return { name: "accessibilityHint", value: this.firstPositionalArg(args, scope) };
      case "accessibilityAddTraits":
        return { name: "accessibilityAddTraits", value: this.accessibilityTokenValue(args, scope) };
      case "accessibilityElement":
        return {
          name: "accessibilityElement",
          value: this.accessibilityElementChildrenValue(args, scope),
        };
      case "accessibilityAction":
        return {
          name: "accessibilityAction",
          value: this.accessibilityActionValue(args, scope),
          action: this.parseCmuxAction(trailingBody ?? "", scope),
          boolValue: trailingBody !== undefined && trailingBody.trim() !== "",
        };
      case "accessibilityActivationPoint": {
        const pointExpression = this.firstPositionalExpression(args);
        const point = this.swiftPointValue(pointExpression, args, scope);
        return {
          name: "accessibilityActivationPoint",
          value: this.swiftUnitPointToken(pointExpression, scope),
          ...(point.x !== undefined ? { x: point.x } : {}),
          ...(point.y !== undefined ? { y: point.y } : {}),
        };
      }
      case "accessibilityRepresentation":
        return {
          name: "accessibilityRepresentation",
          children: this.parseStatements(trailingBody ?? "", scope),
          boolValue: trailingBody !== undefined && trailingBody.trim() !== "",
        };
      case "accessibilitySortPriority":
        return {
          name: "accessibilitySortPriority",
          value: String(this.toNumber(this.splitTopLevel(args, ",")[0], scope) ?? 0),
        };
      case "redacted":
        return { name: "redacted", value: this.namedArg(args, "reason") ?? this.firstPositionalArg(args, scope) };
      case "privacySensitive":
        return { name: "privacySensitive" };
      case "unredacted":
        return { name: "unredacted" };
      case "listRowBackground":
        return { name: "listRowBackground", value: this.firstPositionalArg(args, scope) };
      case "listRowSeparator":
        return {
          name: "listRowSeparator",
          value: this.firstPositionalArg(args, scope),
        };
      case "labelsHidden":
        return { name: "labelsHidden", boolValue: true };
      case "labelStyle":
        return { name: "labelStyle", value: this.firstPositionalArg(args, scope) };
      case "listStyle":
        return { name: "listStyle", value: this.firstPositionalArg(args, scope) };
      case "menuStyle":
        return { name: "menuStyle", value: this.firstPositionalArg(args, scope) };
      case "controlGroupStyle":
        return { name: "controlGroupStyle", value: this.firstPositionalArg(args, scope) };
      case "groupBoxStyle":
        return { name: "groupBoxStyle", value: this.firstPositionalArg(args, scope) };
      case "tabViewStyle":
        return { name: "tabViewStyle", value: this.swiftTypeToken(this.splitTopLevel(args, ",")[0]) };
      case "pickerStyle":
        return { name: "pickerStyle", value: this.firstPositionalArg(args, scope) };
      case "toggleStyle":
        return { name: "toggleStyle", value: this.firstPositionalArg(args, scope) };
      case "textFieldStyle":
        return { name: "textFieldStyle", value: this.firstPositionalArg(args, scope) };
      case "scrollContentBackground":
        return {
          name: "scrollContentBackground",
          value: this.firstPositionalArg(args, scope),
        };
      case "scrollIndicators":
        return {
          name: "scrollIndicators",
          value: this.firstPositionalArg(args, scope),
          secondaryValue: this.namedArg(args, "axes")?.replace(/^\./, ""),
        };
      case "scrollClipDisabled":
        return {
          name: "scrollClipDisabled",
          boolValue:
            args.trim() === ""
              ? true
              : this.evalCondition(this.splitTopLevel(args, ",")[0] ?? "true", scope),
        };
      case "scrollTargetBehavior":
        return {
          name: "scrollTargetBehavior",
          value: this.firstPositionalArg(args, scope),
        };
      case "scrollTargetLayout":
        return {
          name: "scrollTargetLayout",
          boolValue:
            args.trim() === ""
              ? true
              : this.evalCondition(this.namedArg(args, "isEnabled") ?? this.splitTopLevel(args, ",")[0] ?? "true", scope),
        };
      case "scrollBounceBehavior":
        return {
          name: "scrollBounceBehavior",
          value: this.firstPositionalArg(args, scope),
          secondaryValue: this.namedArg(args, "axes")?.replace(/^\./, ""),
        };
      case "scrollDisabled":
        return {
          name: "scrollDisabled",
          boolValue:
            args.trim() === ""
              ? true
              : this.evalCondition(this.splitTopLevel(args, ",")[0] ?? "true", scope),
        };
      case "scrollPosition": {
        const idExpression = this.namedArg(args, "id");
        const bindingKey =
          idExpression === undefined ? undefined : this.stateBindingKey(idExpression, scope);
        const initialAnchor = this.swiftUnitPointToken(this.namedArg(args, "initialAnchor"), scope);
        return {
          name: "scrollPosition",
          value:
            idExpression === undefined
              ? initialAnchor
              : bindingKey ?? this.evalExpression(idExpression, scope),
          secondaryValue:
            this.swiftUnitPointToken(this.namedArg(args, "anchor"), scope) ?? initialAnchor,
          ...(bindingKey !== undefined ? { stateBindingKey: bindingKey } : {}),
        };
      }
      case "defaultScrollAnchor":
        return {
          name: "defaultScrollAnchor",
          value: this.swiftUnitPointToken(this.firstPositionalExpression(args), scope),
        };
      case "preferredColorScheme":
        return {
          name: "preferredColorScheme",
          value: this.swiftColorSchemeToken(this.firstPositionalExpression(args), scope),
        };
      case "environment": {
        const parts = this.splitTopLevel(args, ",");
        const key = this.swiftEnvironmentKeyToken(parts[0], scope);
        if (key === undefined) return null;
        return {
          name: "environment",
          value: key,
          secondaryValue:
            key === "colorScheme"
              ? this.swiftColorSchemeToken(parts[1], scope)
              : this.swiftLayoutDirectionToken(parts[1], scope),
        };
      }
      case "resizable":
        return {
          name: "resizable",
          secondaryValue: this.swiftResizableModeToken(this.namedArg(args, "resizingMode"), scope),
          ...this.swiftEdgeInsetsValue(this.namedArg(args, "capInsets"), scope),
        };
      case "renderingMode":
        return { name: "renderingMode", value: this.firstPositionalArg(args, scope) };
      case "interpolation":
        return { name: "interpolation", value: this.firstPositionalArg(args, scope) };
      case "antialiased":
        return {
          name: "antialiased",
          boolValue:
            args.trim() === ""
              ? true
              : this.evalCondition(this.splitTopLevel(args, ",")[0] ?? args, scope),
        };
      case "flipsForRightToLeftLayoutDirection":
        return {
          name: "flipsForRightToLeftLayoutDirection",
          boolValue:
            args.trim() === ""
              ? true
              : this.evalCondition(this.firstPositionalExpression(args) ?? "true", scope),
        };
      case "imageScale":
        return { name: "imageScale", value: this.firstPositionalArg(args, scope) };
      case "symbolRenderingMode":
        return {
          name: "symbolRenderingMode",
          value: this.firstPositionalArg(args, scope),
        };
      case "symbolVariant":
        return { name: "symbolVariant", value: this.firstPositionalArg(args, scope) };
      default:
        return null;
    }
  }

  private parseEventHandler(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftEventHandler | null {
    const eventName = this.firstPositionalArg(args, scope) || this.namedArgValue(args, "name", scope);
    const eventCategory = this.namedArgValue(args, "category", scope);
    const assignments = this.parseEventAssignments(trailingBody, scope);
    const action = this.parseCmuxAction(trailingBody, scope);
    if (!eventName && !eventCategory) {
      this.warnings.push("Unsupported onEvent handler without a name or category");
      return null;
    }
    if (assignments.length === 0 && action === undefined) {
      this.warnings.push(`Unsupported onEvent handler body '${trailingBody.trim().slice(0, 48)}'`);
      return null;
    }
    return {
      ...(eventName ? { eventName } : {}),
      ...(eventCategory ? { eventCategory } : {}),
      assignments,
      ...(action !== undefined ? { action } : {}),
    };
  }

  private parseLocalHandler(
    body: string,
    scope: SwiftParseScope,
    modifierName: CustomSidebarSwiftLocalHandlerModifierName,
  ): CustomSidebarSwiftLocalHandler | null {
    const assignments = this.parseEventAssignments(body, scope);
    const action = this.parseCmuxAction(body, scope);
    if (assignments.length === 0 && action === undefined) {
      this.warnings.push(`Unsupported ${modifierName} handler body '${body.trim().slice(0, 48)}'`);
      return null;
    }
    return {
      assignments,
      ...(action !== undefined ? { action } : {}),
    };
  }

  private parseEventAssignments(
    body: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftStateAssignment[] {
    const assignments: CustomSidebarSwiftStateAssignment[] = [];
    let index = 0;
    while (index < body.length && assignments.length < 25) {
      index = this.skipWhitespaceAndSeparators(body, index);
      if (index >= body.length) break;
      const statement = this.readSimpleStatement(body, index);
      index = statement.end;
      if (statement.text.trim().startsWith("cmux")) continue;
      const match = statement.text.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*([\s\S]+)$/);
      if (!match) continue;
      const key = match[1] ?? "";
      if (scope.__stateKeys?.[key] !== true) continue;
      assignments.push({
        key,
        value: this.swiftStateValue(this.evalValue(match[2] ?? "", scope)),
      });
    }
    return assignments;
  }

  private swiftStateValue(value: unknown): SwiftSidebarStateValue {
    if (
      typeof value === "string" ||
      typeof value === "number" ||
      typeof value === "boolean" ||
      value === null
    ) {
      return value;
    }
    if (Array.isArray(value)) {
      return value.slice(0, 250).map((entry) => this.swiftStateValue(entry));
    }
    if (typeof value === "object" && value !== null) {
      return Object.fromEntries(
        Object.entries(value as Record<string, unknown>).map(([key, entry]) => [
          key,
          this.swiftStateValue(entry),
        ]),
      );
    }
    return value === undefined ? null : String(value);
  }

  private namedArgValue(
    args: string,
    name: string,
    scope: SwiftParseScope,
  ): string | undefined {
    const value = this.namedArg(args, name);
    return value === undefined ? undefined : this.evalExpression(value, scope);
  }

  private parseCmuxAction(
    source: string,
    scope: SwiftParseScope,
  ): CustomSidebarJsonAction | undefined {
    const cmuxIndex = source.indexOf("cmux");
    if (cmuxIndex < 0) return undefined;
    const call = this.readCall(source.slice(cmuxIndex));
    if (call === null || call.name !== "cmux") return undefined;
    const args = this.splitTopLevel(call.args, ",");
    const method = this.evalExpression(args[0] ?? "", scope).trim();
    if (!method) return undefined;
    const params: Record<string, unknown> = {};
    for (const arg of args.slice(1)) {
      const colon = this.topLevelIndexOf(arg, ":");
      if (colon < 0) continue;
      const key = arg.slice(0, colon).trim();
      params[key] = this.evalValue(arg.slice(colon + 1), scope);
    }
    return { method, params };
  }

  private userFunction(
    name: string,
    scope: SwiftParseScope,
  ): SwiftUserFunction | undefined {
    return scope.__functions?.[name];
  }

  private invokeViewFunction(
    name: string,
    args: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const functionScope = this.scopeWithFunctionArgs(name, args, scope);
    const nodes = this.parseStatements(
      this.userFunction(name, scope)?.body ?? "",
      functionScope,
    );
    if (nodes.length === 1 && nodes[0] !== undefined) return nodes[0];
    return { kind: "group", children: nodes };
  }

  private invokeValueFunction(
    name: string,
    args: string,
    scope: SwiftParseScope,
  ): unknown {
    const fn = this.userFunction(name, scope);
    if (fn === undefined || fn.returnsView) return undefined;
    let currentScope = this.scopeWithFunctionArgs(name, args, scope);
    let index = 0;
    let result: unknown;
    while (index < fn.body.length) {
      index = this.skipWhitespaceAndSeparators(fn.body, index);
      if (index >= fn.body.length) break;
      if (fn.body.startsWith("let ", index) || fn.body.startsWith("let\t", index)) {
        const parsed = this.parseLetBinding(fn.body, index, currentScope);
        currentScope = parsed.scope;
        index = parsed.end;
        continue;
      }
      if (fn.body.startsWith("return ", index) || fn.body.startsWith("return\t", index)) {
        const statement = this.readSimpleStatement(
          fn.body,
          this.skipWhitespaceAndSeparators(fn.body, index + "return".length),
        );
        return this.evalValue(statement.text, currentScope);
      }
      const statement = this.readSimpleStatement(fn.body, index);
      result = this.evalValue(statement.text, currentScope);
      index = statement.end;
    }
    return result;
  }

  private scopeWithFunctionArgs(
    name: string,
    args: string,
    scope: SwiftParseScope,
  ): SwiftParseScope {
    const fn = this.userFunction(name, scope);
    if (fn === undefined) return scope;
    let next = { ...scope } as SwiftParseScope;
    const values = this.splitTopLevel(args, ",").map((arg) => {
      const colon = this.topLevelIndexOf(arg, ":");
      return this.evalValue(colon >= 0 ? arg.slice(colon + 1) : arg, scope);
    });
    fn.params.forEach((param, index) => {
      next = this.scopeWithLoopValue(next, param, values[index]);
    });
    return next;
  }

  private parseFunctionParams(params: string): string[] {
    return this.splitTopLevel(params, ",")
      .map((param) => {
        const colon = this.topLevelIndexOf(param, ":");
        const beforeType = param.slice(0, colon < 0 ? param.length : colon).trim();
        const tokens = beforeType.split(/\s+/).filter((token) => token && token !== "_");
        return tokens.at(-1) ?? "";
      })
      .filter(Boolean);
  }

  private evalCondition(condition: string, scope: SwiftParseScope): boolean {
    return this.truthy(this.evalValue(condition, scope));
  }

  private evalBindingCondition(expression: string, scope: SwiftParseScope): boolean {
    const value = this.evalBindingValue(expression, scope);
    if (typeof value === "boolean") return value;
    if (typeof value === "number") return value !== 0;
    if (typeof value === "string") return value !== "" && value !== "false";
    return value !== undefined && value !== null;
  }

  private evalBindingExpression(expression: string, scope: SwiftParseScope): string {
    const value = this.evalBindingValue(expression, scope);
    return value === undefined || value === null ? "" : String(value);
  }

  private evalBindingValue(expression: string, scope: SwiftParseScope): unknown {
    return this.evalValue(this.unwrapBindingExpression(expression), scope);
  }

  private unwrapBindingExpression(expression: string): string {
    let trimmed = expression.trim();
    if (trimmed.startsWith("$")) trimmed = trimmed.slice(1).trim();
    for (const prefix of [".constant", "Binding.constant"]) {
      if (!trimmed.startsWith(`${prefix}(`)) continue;
      const open = trimmed.indexOf("(");
      const balanced = this.readBalanced(trimmed, open, "(", ")");
      if (balanced !== null && balanced.end === trimmed.length) {
        return balanced.content.trim();
      }
    }
    return trimmed;
  }

  private stateBindingKey(
    expression: string,
    scope: SwiftParseScope,
  ): string | undefined {
    const trimmed = expression.trim();
    if (!trimmed.startsWith("$")) return undefined;
    return this.stateBindingPath(trimmed.slice(1).trim(), scope);
  }

  private stateBindingPath(expression: string, scope: SwiftParseScope): string | undefined {
    const first = expression.match(/^([A-Za-z_][A-Za-z0-9_]*)/);
    if (!first) return undefined;
    const firstName = first[1] ?? "";
    const base =
      scope.__stateKeys?.[firstName] === true ? firstName : scope.__bindingKeys?.[firstName];
    if (base === undefined) return undefined;
    let index = first[0].length;
    let key = base;
    while (index < expression.length) {
      const char = expression[index];
      if (char === ".") {
        const member = expression.slice(index + 1).match(/^([A-Za-z_][A-Za-z0-9_]*)/);
        if (!member) return undefined;
        key += `.${member[1] ?? ""}`;
        index += 1 + member[0].length;
        continue;
      }
      if (char === "[") {
        const balanced = this.readBalanced(expression, index, "[", "]");
        if (balanced === null) return undefined;
        const subscript = this.evalValue(balanced.content, scope);
        if (typeof subscript === "number" && Number.isInteger(subscript)) {
          key += `[${subscript}]`;
        } else if (typeof subscript === "string" && /^[A-Za-z_][A-Za-z0-9_]*$/.test(subscript)) {
          key += `.${subscript}`;
        } else if (typeof subscript === "string") {
          key += `[${JSON.stringify(subscript)}]`;
        } else {
          return undefined;
        }
        index = balanced.end;
        continue;
      }
      if (/\s/.test(char ?? "")) {
        index += 1;
        continue;
      }
      return undefined;
    }
    return key;
  }

  private parseRangeBounds(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): { lower: number; upper: number } | undefined {
    if (expression === undefined) return undefined;
    const closed = expression.match(/^(.+?)\s*\.\.\.\s*(.+)$/);
    const halfOpen = expression.match(/^(.+?)\s*\.\.<\s*(.+)$/);
    const range = closed ?? halfOpen;
    if (!range) return undefined;
    const lower = this.toNumber(range[1], scope);
    const upper = this.toNumber(range[2], scope);
    return lower === undefined || upper === undefined ? undefined : { lower, upper };
  }

  private evalExpression(expression: string, scope: SwiftParseScope): string {
    const value = this.evalValue(expression, scope);
    const swiftValue = this.swiftGeometryDisplayValue(value);
    if (swiftValue !== undefined) return swiftValue;
    if (value !== undefined && value !== null) return String(value);
    return expression.trim().replace(/^\./, "");
  }

  private parseText(args: string, scope: SwiftParseScope): CustomSidebarSwiftNode {
    const textStyle = this.swiftTextStyleToken(this.namedArg(args, "style"));
    const timerIntervalArg = this.namedArg(args, "timerInterval");
    if (timerIntervalArg !== undefined) {
      const interval = this.evalDateInterval(timerIntervalArg, scope);
      const countsDown = this.evalCondition(this.namedArg(args, "countsDown") ?? "true", scope);
      return {
        kind: "text",
        text:
          interval === undefined
            ? ""
            : this.formatSwiftTimerInterval(interval, countsDown),
        textStyle: "timer",
        ...(interval !== undefined
          ? {
              timerIntervalStartMs: interval.start.epochMs,
              timerIntervalEndMs: interval.end.epochMs,
            }
          : {}),
        timerCountsDown: countsDown,
      };
    }
    const verbatim = this.namedArg(args, "verbatim");
    if (verbatim !== undefined) {
      return { kind: "text", text: this.evalExpression(verbatim, scope) };
    }
    const markdown = this.namedArg(args, "markdown");
    if (markdown !== undefined) {
      const text = this.evalExpression(markdown, scope);
      return { kind: "text", text, markdownRuns: this.parseInlineMarkdown(text) };
    }
    const firstArg = this.splitTopLevel(args, ",").find(
      (arg) => this.topLevelIndexOf(arg, ":") < 0,
    );
    const text = this.evalText(args, scope);
    if (textStyle !== undefined) {
      return { kind: "text", text, textStyle };
    }
    if (
      firstArg !== undefined &&
      this.isStaticSwiftStringLiteral(firstArg) &&
      this.containsInlineMarkdown(text)
    ) {
      return { kind: "text", text, markdownRuns: this.parseInlineMarkdown(text) };
    }
    return { kind: "text", text };
  }

  private parseTextDecorationModifier(
    name: "underline" | "strikethrough",
    args: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftModifier {
    const activeArg = this.splitTopLevel(args, ",").find(
      (arg) => arg.trim() !== "" && this.topLevelIndexOf(arg, ":") < 0,
    );
    return {
      name,
      boolValue:
        activeArg === undefined || activeArg.trim() === ""
          ? true
          : this.evalCondition(activeArg, scope),
      value: this.namedArg(args, "color")?.replace(/^\./, ""),
      secondaryValue: this.namedArg(args, "pattern")?.replace(/^\./, ""),
    };
  }

  private parseLabel(args: string, scope: SwiftParseScope): CustomSidebarSwiftNode {
    const titleClosure = this.extractNamedClosure(args, "title");
    const iconClosure = this.extractNamedClosure(args, "icon");
    if (titleClosure !== null || iconClosure !== null) {
      const titleNode = this.parseStatements(titleClosure ?? "", scope).find(
        (node) => node.kind === "text",
      );
      const iconNode = this.parseStatements(iconClosure ?? "", scope).find(
        (node) => node.kind === "image",
      );
      return {
        kind: "label",
        text: titleNode?.kind === "text" ? titleNode.text : "",
        ...(iconNode?.kind === "image" ? { systemImage: iconNode.systemName } : {}),
      };
    }
    return {
      kind: "label",
      text: this.firstPositionalArg(args, scope) ?? "",
      systemImage: this.namedStringArg(args, "systemImage", scope),
    };
  }

  private parseImage(args: string, scope: SwiftParseScope): CustomSidebarSwiftNode {
    const systemName = this.namedStringArg(args, "systemName", scope);
    if (systemName !== undefined) {
      return { kind: "image", systemName };
    }
    const assetName =
      this.namedStringArg(args, "decorative", scope) ??
      this.firstPositionalArg(args, scope);
    const name = assetName?.trim() || "missing";
    return {
      kind: "assetImage",
      name,
      url: safeCustomSidebarAssetUrl(scope.root.assets?.[name] ?? ""),
      decorative: this.namedArg(args, "decorative") !== undefined,
    };
  }

  private parseAsyncImage(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
    labeledTrailingClosures: Record<string, string> = {},
  ): CustomSidebarSwiftNode {
    const url = this.safeRemoteImageUrl(this.namedArg(args, "url") ?? args, scope);
    const contentClosure =
      this.extractNamedClosure(args, "content") ??
      labeledTrailingClosures.content ??
      trailingBody;
    const placeholderClosure =
      this.extractNamedClosure(args, "placeholder") ??
      labeledTrailingClosures.placeholder ??
      null;
    const base: CustomSidebarSwiftNode = { kind: "asyncImage", url };
    const successChildren =
      contentClosure.trim() === ""
        ? undefined
        : this.parseAsyncImageContentChildren(contentClosure, base, scope);
    const placeholderChildren =
      placeholderClosure === null || placeholderClosure.trim() === ""
        ? undefined
        : this.parseStatements(placeholderClosure, scope);
    return {
      ...base,
      ...(successChildren !== undefined ? { successChildren } : {}),
      ...(placeholderChildren !== undefined ? { placeholderChildren } : {}),
    };
  }

  private parseAsyncImageContentChildren(
    closureBody: string,
    imageNode: CustomSidebarSwiftNode,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode[] {
    const closure = this.splitClosureParameter(closureBody);
    const imageParam = closure.params[0] ?? "image";
    const body = closure.body.trim();
    const imageParamPattern = new RegExp(`^${this.escapeRegExp(imageParam)}\\b`);
    if (imageParamPattern.test(body)) {
      return [this.withModifiers(imageNode, body, imageParam.length, scope)];
    }
    return this.parseStatements(body, scope);
  }

  private parsePath(args: string, scope: SwiftParseScope): CustomSidebarSwiftNode {
    const roundedRectExpression = this.namedArg(args, "roundedRect");
    const ellipseExpression = this.namedArg(args, "ellipseIn");
    const rect =
      this.swiftRectValue(roundedRectExpression, scope) ??
      this.swiftRectValue(ellipseExpression, scope);
    const rectFields =
      rect === undefined
        ? {}
        : {
            pathX: rect.x,
            pathY: rect.y,
            pathWidth: rect.width,
            pathHeight: rect.height,
          };
    if (ellipseExpression !== undefined) {
      return {
        kind: "shape",
        shape: "pathEllipse",
        ...rectFields,
      };
    }
    return {
      kind: "shape",
      shape: "pathRoundedRect",
      radius: this.toNumber(this.namedArg(args, "cornerRadius"), scope),
      ...rectFields,
    };
  }

  private parseLabeledContent(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const children = this.parseStatements(trailingBody, scope);
    const valueArg = this.namedArg(args, "value");
    return {
      kind: "labeledContent",
      title: this.firstPositionalArg(args, scope),
      value: valueArg === undefined ? undefined : this.evalExpression(valueArg, scope),
      children,
    };
  }

  private parseGroupBox(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
    labeledTrailingClosures: Record<string, string> = {},
  ): CustomSidebarSwiftNode {
    const labelClosure =
      this.extractNamedClosure(args, "label") ??
      labeledTrailingClosures.label ??
      null;
    const contentClosure =
      this.extractNamedClosure(args, "content") ??
      labeledTrailingClosures.content ??
      trailingBody;
    const labelChildren =
      labelClosure === null || labelClosure.trim() === ""
        ? []
        : this.parseStatements(labelClosure, scope);
    return {
      kind: "groupBox",
      title: this.firstPositionalArg(args, scope),
      labelChildren,
      children: this.parseStatements(contentClosure, scope),
    };
  }

  private parseDisclosureGroup(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
    labeledTrailingClosures: Record<string, string> = {},
  ): CustomSidebarSwiftNode {
    const isExpandedBinding = this.namedArg(args, "isExpanded");
    const stateBindingKey = this.stateBindingKey(isExpandedBinding ?? "", scope);
    const labelClosure =
      this.extractNamedClosure(args, "label") ??
      labeledTrailingClosures.label ??
      null;
    const contentClosure =
      this.extractNamedClosure(args, "content") ??
      labeledTrailingClosures.content ??
      trailingBody;
    return {
      kind: "disclosureGroup",
      title: this.firstPositionalArg(args, scope),
      isExpanded:
        isExpandedBinding === undefined
          ? true
          : this.evalBindingCondition(isExpandedBinding, scope),
      ...(stateBindingKey !== undefined ? { stateBindingKey } : {}),
      labelChildren:
        labelClosure === null || labelClosure.trim() === ""
          ? []
          : this.parseStatements(labelClosure, scope),
      children: this.parseStatements(contentClosure, scope),
    };
  }

  private evalText(args: string, scope: SwiftParseScope): string {
    const format = this.namedArg(args, "format");
    if (format !== undefined) {
      const valueArg = this.splitTopLevel(args, ",").find(
        (arg) => this.topLevelIndexOf(arg, ":") < 0,
      );
      if (valueArg !== undefined) {
        return this.formatSwiftValue(this.evalValue(valueArg, scope), format, scope);
      }
    }
    const style = this.swiftTextStyleToken(this.namedArg(args, "style"));
    if (style !== undefined) {
      const valueArg = this.firstPositionalExpression(args);
      const value = valueArg === undefined ? undefined : this.evalValue(valueArg, scope);
      if (this.isSwiftDate(value)) {
        return this.formatSwiftTextDateStyle(value, style);
      }
    }
    return this.evalExpression(args, scope);
  }

  private swiftTextStyleToken(expression: string | undefined): string | undefined {
    const token = expression?.trim().replace(/^\./, "");
    if (
      token === "date" ||
      token === "time" ||
      token === "relative" ||
      token === "timer" ||
      token === "offset"
    ) {
      return token;
    }
    return undefined;
  }

  private isStaticSwiftStringLiteral(expression: string): boolean {
    const trimmed = expression.trim();
    return (
      trimmed.startsWith('"') &&
      trimmed.endsWith('"') &&
      !trimmed.includes("\\(")
    );
  }

  private containsInlineMarkdown(text: string): boolean {
    return this.parseInlineMarkdown(text).some((run) => run.kind !== "text");
  }

  private parseInlineMarkdown(text: string): CustomSidebarSwiftTextRun[] {
    const runs: CustomSidebarSwiftTextRun[] = [];
    let scanIndex = 0;
    let textStart = 0;
    const pushText = (end: number) => {
      if (end > textStart) {
        runs.push({ kind: "text", text: text.slice(textStart, end) });
      }
    };

    while (scanIndex < text.length) {
      if (text.startsWith("**", scanIndex)) {
        const end = text.indexOf("**", scanIndex + 2);
        if (end > scanIndex + 2) {
          pushText(scanIndex);
          runs.push({ kind: "strong", text: text.slice(scanIndex + 2, end) });
          scanIndex = end + 2;
          textStart = scanIndex;
          continue;
        }
      }

      if (text[scanIndex] === "`") {
        const end = text.indexOf("`", scanIndex + 1);
        if (end > scanIndex + 1) {
          pushText(scanIndex);
          runs.push({ kind: "code", text: text.slice(scanIndex + 1, end) });
          scanIndex = end + 1;
          textStart = scanIndex;
          continue;
        }
      }

      if (text[scanIndex] === "[") {
        const labelEnd = text.indexOf("]", scanIndex + 1);
        if (labelEnd > scanIndex + 1 && text[labelEnd + 1] === "(") {
          const hrefEnd = text.indexOf(")", labelEnd + 2);
          if (hrefEnd > labelEnd + 2) {
            pushText(scanIndex);
            runs.push({
              kind: "link",
              text: text.slice(scanIndex + 1, labelEnd),
              href: this.safeMarkdownHref(text.slice(labelEnd + 2, hrefEnd)),
            });
            scanIndex = hrefEnd + 1;
            textStart = scanIndex;
            continue;
          }
        }
      }

      if (
        text[scanIndex] === "*" &&
        text[scanIndex - 1] !== "*" &&
        text[scanIndex + 1] !== "*"
      ) {
        const end = this.findSingleAsterisk(text, scanIndex + 1);
        if (end > scanIndex + 1) {
          pushText(scanIndex);
          runs.push({ kind: "emphasis", text: text.slice(scanIndex + 1, end) });
          scanIndex = end + 1;
          textStart = scanIndex;
          continue;
        }
      }

      scanIndex += 1;
    }

    pushText(text.length);
    return runs.length > 0 ? runs : [{ kind: "text", text }];
  }

  private findSingleAsterisk(text: string, startIndex: number): number {
    let index = startIndex;
    while (index < text.length) {
      if (
        text[index] === "*" &&
        text[index - 1] !== "*" &&
        text[index + 1] !== "*"
      ) {
        return index;
      }
      index += 1;
    }
    return -1;
  }

  private safeMarkdownHref(href: string): string {
    return /^https?:\/\//i.test(href) ? href : "#";
  }

  private safeRemoteImageUrl(
    expression: string,
    scope: SwiftParseScope,
  ): string | undefined {
    const unwrapped = this.unwrapUrlInitializer(expression);
    const url = this.evalExpression(unwrapped, scope).trim();
    return /^https?:\/\//i.test(url) ? url : undefined;
  }

  private safeExternalLinkUrl(
    expression: string,
    scope: SwiftParseScope,
  ): string | undefined {
    const unwrapped = this.unwrapUrlInitializer(expression);
    const url = this.evalExpression(unwrapped, scope).trim();
    return /^https?:\/\//i.test(url) ? url : undefined;
  }

  private unwrapUrlInitializer(expression: string): string {
    const trimmed = expression.trim().replace(/[!?]\s*$/, "");
    if (!trimmed.startsWith("URL(")) return trimmed;
    const call = this.readCall(trimmed);
    if (call === null || call.end !== trimmed.length || call.name !== "URL") {
      return trimmed;
    }
    return this.namedArg(call.args, "string") ?? this.splitTopLevel(call.args, ",")[0] ?? "";
  }

  private evalValue(expression: string, scope: SwiftParseScope): unknown {
    const trimmed = this.unwrapParenthesizedExpression(expression.trim());
    const ternary = this.splitTernary(trimmed);
    if (ternary !== null) {
      return this.evalCondition(ternary.condition, scope)
        ? this.evalValue(ternary.truthy, scope)
        : this.evalValue(ternary.falsy, scope);
    }
    const logical = this.evalLogicalExpression(trimmed, scope);
    if (logical !== undefined) return logical;
    if (trimmed.startsWith("!")) return !this.evalCondition(trimmed.slice(1), scope);
    const comparison = this.evalComparisonExpression(trimmed, scope);
    if (comparison !== undefined) return comparison;
    if (trimmed.startsWith('"') && trimmed.endsWith('"')) {
      return this.interpolateSwiftString(this.unquote(trimmed), scope);
    }
    if (trimmed === "nil" || trimmed === "null") return null;
    if (trimmed === "true") return true;
    if (trimmed === "false") return false;
    if (/^-?\d+(?:\.\d+)?$/.test(trimmed)) return Number(trimmed);
    const arithmetic = this.evalArithmeticExpression(trimmed, scope);
    if (arithmetic !== undefined) return arithmetic;
    if (trimmed.startsWith("[") && trimmed.endsWith("]")) {
      const balanced = this.readBalanced(trimmed, 0, "[", "]");
      if (balanced !== null && balanced.end === trimmed.length) {
        return this.evalCollectionLiteral(balanced.content, scope);
      }
    }
    const dottedLiteral = this.evalDottedSwiftGeometryLiteral(trimmed, scope);
    if (dottedLiteral !== undefined) return dottedLiteral;
    const functionCall = this.readCall(trimmed);
    if (functionCall !== null && functionCall.end === trimmed.length) {
      const builtinValue = this.evalBuiltinFunction(
        functionCall.name,
        functionCall.args,
        scope,
      );
      if (builtinValue !== undefined) return builtinValue;
      const functionValue = this.invokeValueFunction(
        functionCall.name,
        functionCall.args,
        scope,
      );
      if (functionValue !== undefined) return functionValue;
    }
    const resolved = this.resolvePath(trimmed, scope);
    if (resolved !== undefined) return resolved;
    return trimmed.replace(/^\./, "");
  }

  private unwrapParenthesizedExpression(expression: string): string {
    let trimmed = expression.trim();
    while (trimmed.startsWith("(")) {
      const balanced = this.readBalanced(trimmed, 0, "(", ")");
      if (balanced === null || balanced.end !== trimmed.length) break;
      trimmed = balanced.content.trim();
    }
    return trimmed;
  }

  private evalLogicalExpression(expression: string, scope: SwiftParseScope): boolean | undefined {
    const orIndex = this.topLevelOperatorIndex(expression, ["||"]);
    if (orIndex >= 0) {
      return (
        this.evalCondition(expression.slice(0, orIndex), scope) ||
        this.evalCondition(expression.slice(orIndex + 2), scope)
      );
    }
    const andIndex = this.topLevelOperatorIndex(expression, ["&&"]);
    if (andIndex >= 0) {
      return (
        this.evalCondition(expression.slice(0, andIndex), scope) &&
        this.evalCondition(expression.slice(andIndex + 2), scope)
      );
    }
    return undefined;
  }

  private evalComparisonExpression(
    expression: string,
    scope: SwiftParseScope,
  ): boolean | undefined {
    const operators = ["==", "!=", ">=", "<=", ">", "<"];
    const index = this.topLevelOperatorIndex(expression, operators);
    if (index < 0) return undefined;
    const operator = operators.find((candidate) => expression.startsWith(candidate, index));
    if (operator === undefined) return undefined;
    const left = this.evalComparisonOperand(expression.slice(0, index), scope);
    const right = this.evalComparisonOperand(expression.slice(index + operator.length), scope);
    const nullishEqual = left == null && right == null;
    if (operator === "==") return nullishEqual || left === right;
    if (operator === "!=") return !(nullishEqual || left === right);
    const leftNumber = Number(left);
    const rightNumber = Number(right);
    const comparable =
      Number.isFinite(leftNumber) && Number.isFinite(rightNumber)
        ? [leftNumber, rightNumber]
        : [String(left), String(right)];
    switch (operator) {
      case ">=":
        return comparable[0] >= comparable[1];
      case "<=":
        return comparable[0] <= comparable[1];
      case ">":
        return comparable[0] > comparable[1];
      case "<":
        return comparable[0] < comparable[1];
      default:
        return undefined;
    }
  }

  private evalArithmeticExpression(expression: string, scope: SwiftParseScope): unknown {
    const additiveIndex = this.topLevelOperatorIndex(expression, ["+", "-"], true);
    if (additiveIndex >= 0) {
      const operator = expression[additiveIndex];
      const left = this.evalValue(expression.slice(0, additiveIndex), scope);
      const right = this.evalValue(expression.slice(additiveIndex + 1), scope);
      if (operator === "+" && (typeof left === "string" || typeof right === "string")) {
        return `${left ?? ""}${right ?? ""}`;
      }
      const leftNumber = Number(left);
      const rightNumber = Number(right);
      if (!Number.isFinite(leftNumber) || !Number.isFinite(rightNumber)) return undefined;
      return operator === "+" ? leftNumber + rightNumber : leftNumber - rightNumber;
    }
    const multiplicativeIndex = this.topLevelOperatorIndex(expression, ["*", "/", "%"], true);
    if (multiplicativeIndex >= 0) {
      const operator = expression[multiplicativeIndex];
      const left = Number(this.evalValue(expression.slice(0, multiplicativeIndex), scope));
      const right = Number(this.evalValue(expression.slice(multiplicativeIndex + 1), scope));
      if (!Number.isFinite(left) || !Number.isFinite(right)) return undefined;
      if ((operator === "/" || operator === "%") && right === 0) return 0;
      if (operator === "*") return left * right;
      return operator === "/" ? left / right : left % right;
    }
    return undefined;
  }

  private truthy(value: unknown): boolean {
    if (typeof value === "boolean") return value;
    if (typeof value === "number") return value !== 0;
    if (typeof value === "string") return value !== "" && value !== "false";
    return value !== undefined && value !== null;
  }

  private evalComparisonOperand(expression: string, scope: SwiftParseScope): unknown {
    const trimmed = this.unwrapParenthesizedExpression(expression.trim());
    if (trimmed === "nil" || trimmed === "null") return null;
    const directPath =
      /^[A-Za-z_][A-Za-z0-9_]*(?:[.\[].*)?$/.test(trimmed) &&
      this.readCall(trimmed)?.end !== trimmed.length;
    if (directPath) {
      return this.resolvePath(trimmed, scope);
    }
    return this.evalValue(trimmed, scope);
  }

  private resolvePath(path: string, scope: SwiftParseScope): unknown {
    const normalized = path.trim();
    if (!normalized) return undefined;
    if (/^-?\d+(?:\.\d+)?$/.test(normalized)) return Number(normalized);
    if (normalized === "true") return true;
    if (normalized === "false") return false;
    const arrayCall = this.readCall(normalized);
    if (arrayCall?.name === "Array" && arrayCall.end === normalized.length) {
      return this.resolvePath(arrayCall.args, scope);
    }
    const parts = this.splitTopLevel(normalized, ".").filter(Boolean);
    let value: unknown = this.resolvePathPart(parts[0] ?? "", scope);
    for (const part of parts.slice(1)) {
      value = this.resolveMember(value, part, scope);
    }
    return value;
  }

  private resolvePathPart(part: string, scope: SwiftParseScope): unknown {
    const call = this.readCall(part);
    if (call !== null && call.end === part.trim().length) {
      return this.evalBuiltinFunction(call.name, call.args, scope);
    }
    const bracket = part.match(/^([A-Za-z_][A-Za-z0-9_]*)\[(.+)\]$/);
    if (bracket) {
      const base =
        (scope as unknown as Record<string, unknown>)[bracket[1] ?? ""] ??
        (scope.root as unknown as Record<string, unknown>)[bracket[1] ?? ""];
      return this.resolveSubscript(base, bracket[2] ?? "", scope);
    }
    return (
      (scope as unknown as Record<string, unknown>)[part] ??
      (scope.root as unknown as Record<string, unknown>)[part]
    );
  }

  private resolveMember(value: unknown, part: string, scope: SwiftParseScope): unknown {
    if (Array.isArray(value)) {
      if (part === "count") return value.length;
      if (part === "indices") return value.map((_entry, index) => index);
      if (part === "first") return value[0];
      if (part === "last") return value.at(-1);
      const callWithTrailingClosure = part.match(
        /^([A-Za-z_][A-Za-z0-9_]*)\(([\s\S]*)\)\s*\{([\s\S]*)\}$/,
      );
      if (callWithTrailingClosure) {
        return this.resolveArrayMethod(
          value,
          callWithTrailingClosure[1] ?? "",
          `${callWithTrailingClosure[2] ?? ""}, {${callWithTrailingClosure[3] ?? ""}}`,
          scope,
        );
      }
      const method = part.match(/^([A-Za-z_][A-Za-z0-9_]*)\((.*)\)$/);
      if (method) {
        if (method[1] === "formatted") {
          return this.formatSwiftValue(value, method[2] ?? "", scope);
        }
        return this.resolveArrayMethod(value, method[1] ?? "", method[2] ?? "", scope);
      }
      const closureMethod = part.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*\{([\s\S]*)\}$/);
      if (closureMethod) {
        return this.resolveArrayMethod(
          value,
          closureMethod[1] ?? "",
          `{${closureMethod[2] ?? ""}}`,
          scope,
        );
      }
    }
    if (typeof value === "number") {
      const method = part.match(/^([A-Za-z_][A-Za-z0-9_]*)\((.*)\)$/);
      if (method?.[1] === "formatted") {
        return this.formatSwiftValue(value, method[2] ?? "", scope);
      }
    }
    if (typeof value === "string") {
      if (part === "count") return Array.from(value).length;
      const method = part.match(/^([A-Za-z_][A-Za-z0-9_]*)\((.*)\)$/);
      if (method) {
        return this.resolveStringMethod(value, method[1] ?? "", method[2] ?? "", scope);
      }
    }
    if (typeof value !== "object" || value === null) return undefined;
    if (this.isSwiftDate(value)) {
      const method = part.match(/^([A-Za-z_][A-Za-z0-9_]*)\((.*)\)$/);
      if (method?.[1] === "formatted") {
        return this.formatSwiftValue(value, method[2] ?? "", scope);
      }
    }
    if (this.isSwiftMeasurement(value)) {
      const method = part.match(/^([A-Za-z_][A-Za-z0-9_]*)\((.*)\)$/);
      if (method?.[1] === "formatted") {
        return this.formatSwiftValue(value, method[2] ?? "", scope);
      }
    }
    if (this.isSwiftAngle(value)) {
      if (part === "degrees") return value.degrees;
      if (part === "radians") return value.degrees * (Math.PI / 180);
    }
    if (this.isSwiftPoint(value) || this.isSwiftUnitPoint(value)) {
      if (part === "x") return value.x;
      if (part === "y") return value.y;
    }
    if (this.isSwiftSize(value)) {
      if (part === "width") return value.width;
      if (part === "height") return value.height;
    }
    if (this.isSwiftRect(value)) {
      if (part === "x" || part === "minX") return value.x;
      if (part === "y" || part === "minY") return value.y;
      if (part === "width") return value.width;
      if (part === "height") return value.height;
      if (part === "maxX") return value.x + value.width;
      if (part === "maxY") return value.y + value.height;
      if (part === "midX") return value.x + value.width / 2;
      if (part === "midY") return value.y + value.height / 2;
      if (part === "origin") return { __swiftPoint: true, x: value.x, y: value.y };
      if (part === "size") return {
        __swiftSize: true,
        width: value.width,
        height: value.height,
      };
    }
    const bracket = part.match(/^([A-Za-z_][A-Za-z0-9_]*)\[(.+)\]$/);
    if (bracket) {
      const child = (value as Record<string, unknown>)[bracket[1] ?? ""];
      return this.resolveSubscript(child, bracket[2] ?? "", scope);
    }
    return (value as Record<string, unknown>)[part];
  }

  private swiftKeyPathParts(expression: string | undefined): string[] | undefined {
    const trimmed = expression?.trim();
    if (trimmed === undefined || trimmed === "") return undefined;
    const withoutRoot = trimmed.replace(/^\\\./, "");
    if (withoutRoot === trimmed) return undefined;
    if (withoutRoot === "self") return [];
    if (!/^[A-Za-z_$][A-Za-z0-9_$]*(?:\.[A-Za-z_$][A-Za-z0-9_$]*)*$/.test(withoutRoot)) {
      return undefined;
    }
    return withoutRoot.split(".");
  }

  private resolveKeyPathValue(
    value: unknown,
    parts: string[],
    scope: SwiftParseScope,
  ): unknown {
    return parts.reduce(
      (current, part) => this.resolveMember(current, part, scope),
      value,
    );
  }

  private compareProjectedValues(left: unknown, right: unknown): number {
    if (typeof left === "number" && typeof right === "number") {
      return left - right;
    }
    if (typeof left === "boolean" && typeof right === "boolean") {
      return Number(left) - Number(right);
    }
    if (left === undefined || left === null) return right === undefined || right === null ? 0 : 1;
    if (right === undefined || right === null) return -1;
    return String(left).localeCompare(String(right));
  }

  private arrayProjection(
    args: string,
    scope: SwiftParseScope,
  ): ((entry: unknown, index: number) => unknown) | undefined {
    const keyPath = this.swiftKeyPathParts(
      this.namedArg(args, "by") ?? this.firstPositionalExpression(args),
    );
    if (keyPath !== undefined) {
      return (entry) => this.resolveKeyPathValue(entry, keyPath, scope);
    }
    const closure = this.arrayMethodClosure(args);
    if (closure === null) return undefined;
    return (entry, index) =>
      this.evalValue(
        closure.body,
        this.scopeWithClosureValues(scope, closure.params, [entry, index]),
      );
  }

  private arrayComparator(
    args: string,
    scope: SwiftParseScope,
  ): ((left: unknown, right: unknown) => number) | undefined {
    const keyPath = this.swiftKeyPathParts(
      this.namedArg(args, "by") ?? this.firstPositionalExpression(args),
    );
    if (keyPath !== undefined) {
      return (left, right) =>
        this.compareProjectedValues(
          this.resolveKeyPathValue(left, keyPath, scope),
          this.resolveKeyPathValue(right, keyPath, scope),
        );
    }
    const closure = this.arrayMethodClosure(args);
    if (closure === null) return undefined;
    return (left, right) => {
      const leftFirst = this.scopeWithClosureValues(scope, closure.params, [left, right]);
      if (this.evalCondition(closure.body, leftFirst)) return -1;
      const rightFirst = this.scopeWithClosureValues(scope, closure.params, [right, left]);
      if (this.evalCondition(closure.body, rightFirst)) return 1;
      return 0;
    };
  }

  private evalCollectionLiteral(content: string, scope: SwiftParseScope): unknown {
    const entries = this.splitTopLevel(content, ",").filter((entry) => entry.trim() !== "");
    const hasDictionaryEntry = entries.some((entry) => this.topLevelIndexOf(entry, ":") >= 0);
    if (!hasDictionaryEntry) {
      return entries.map((entry) => this.evalValue(entry, scope));
    }
    const record: Record<string, unknown> = {};
    for (const entry of entries) {
      const colon = this.topLevelIndexOf(entry, ":");
      if (colon < 0) continue;
      const key = this.evalValue(entry.slice(0, colon), scope);
      record[String(key)] = this.evalValue(entry.slice(colon + 1), scope);
    }
    return record;
  }

  private resolveSubscript(
    value: unknown,
    expression: string,
    scope: SwiftParseScope,
  ): unknown {
    const key = this.evalValue(expression, scope);
    if (Array.isArray(value)) {
      const index = typeof key === "number" ? key : Number(key);
      return Number.isInteger(index) ? value[index] : undefined;
    }
    if (typeof value === "object" && value !== null) {
      return (value as Record<string, unknown>)[String(key)];
    }
    if (typeof value === "string") {
      const index = typeof key === "number" ? key : Number(key);
      return Number.isInteger(index) ? Array.from(value)[index] : undefined;
    }
    return undefined;
  }

  private resolveArrayMethod(
    value: unknown[],
    method: string,
    args: string,
    scope: SwiftParseScope,
  ): unknown {
    const count = this.toNumber(this.splitTopLevel(args, ",")[0], scope);
    switch (method) {
      case "filter": {
        const closure = this.arrayMethodClosure(args);
        if (closure === null) return undefined;
        return value.filter((entry, index) =>
          this.evalCondition(
            closure.body,
            this.scopeWithClosureValues(scope, closure.params, [entry, index]),
          ),
        );
      }
      case "map": {
        const projection = this.arrayProjection(args, scope);
        return projection === undefined
          ? undefined
          : value.map((entry, index) => projection(entry, index));
      }
      case "compactMap": {
        const projection = this.arrayProjection(args, scope);
        return projection === undefined
          ? undefined
          : value
          .map((entry, index) => projection(entry, index))
          .filter((entry) => entry !== undefined && entry !== null);
      }
      case "flatMap": {
        const projection = this.arrayProjection(args, scope);
        if (projection === undefined) return undefined;
        return value.flatMap((entry, index) => {
          const mapped = projection(entry, index);
          return Array.isArray(mapped) ? mapped : [mapped];
        });
      }
      case "reduce": {
        const initial = this.reduceInitialValue(args, scope);
        const closure = this.arrayMethodClosure(args);
        if (closure === null) return undefined;
        return value.reduce(
          (total, entry, index) =>
            this.evalValue(
              closure.body,
              this.scopeWithClosureValues(scope, closure.params, [total, entry, index]),
            ),
          initial,
        );
      }
      case "sorted": {
        const entries = value.slice();
        const comparator = this.arrayComparator(args, scope);
        return comparator === undefined
          ? entries.sort((left, right) => String(left).localeCompare(String(right)))
          : entries.sort(comparator);
      }
      case "min": {
        if (value.length === 0) return undefined;
        const comparator = this.arrayComparator(args, scope);
        if (comparator === undefined) {
          return value
            .slice()
            .sort((left, right) => this.compareProjectedValues(left, right))[0];
        }
        return value.slice().sort(comparator)[0];
      }
      case "max": {
        if (value.length === 0) return undefined;
        const comparator = this.arrayComparator(args, scope);
        if (comparator === undefined) {
          return value
            .slice()
            .sort((left, right) => this.compareProjectedValues(right, left))[0];
        }
        return value.slice().sort((left, right) => comparator(right, left))[0];
      }
      case "allSatisfy": {
        const projection = this.arrayProjection(args, scope);
        if (projection === undefined) return undefined;
        return value.every((entry, index) => this.truthy(projection(entry, index)));
      }
      case "prefix":
        return value.slice(0, Math.max(0, Math.floor(count ?? value.length)));
      case "suffix": {
        const length = Math.max(0, Math.floor(count ?? value.length));
        return length === 0 ? [] : value.slice(-length);
      }
      case "dropFirst":
        return value.slice(Math.max(0, Math.floor(count ?? 1)));
      case "dropLast": {
        const length = Math.max(0, Math.floor(count ?? 1));
        return length === 0 ? value.slice() : value.slice(0, -length);
      }
      case "reversed":
        return value.slice().reverse();
      case "enumerated":
        return value.map((element, offset) => ({ offset, element }));
      case "contains": {
        const needle = this.evalExpression(this.splitTopLevel(args, ",")[0] ?? "", scope);
        return value.some((entry) => String(entry) === needle);
      }
      default:
        return undefined;
    }
  }

  private reduceInitialValue(args: string, scope: SwiftParseScope): unknown {
    const initialArg =
      this.splitTopLevel(args, ",").find((arg) => !arg.trim().startsWith("{")) ?? "";
    const trimmed = initialArg.trim();
    if (trimmed.startsWith("into:")) {
      return this.evalValue(trimmed.slice("into:".length), scope);
    }
    return this.evalValue(trimmed, scope);
  }

  private arrayMethodClosure(
    args: string,
  ): { params: string[]; body: string } | null {
    const trimmed = args.trim();
    if (!trimmed) return null;
    const closureSource = trimmed.startsWith("{")
      ? trimmed
      : this.splitTopLevel(trimmed, ",").find((arg) => arg.trim().startsWith("{"));
    if (closureSource === undefined) return null;
    const open = closureSource.indexOf("{");
    const balanced = this.readBalanced(closureSource, open, "{", "}");
    if (balanced === null) return null;
    return this.splitClosureParameter(balanced.content);
  }

  private scopeWithClosureValues(
    scope: SwiftParseScope,
    params: string[],
    values: unknown[],
  ): SwiftParseScope {
    let next = { ...scope } as SwiftParseScope;
    values.forEach((value, index) => {
      next = { ...next, [`$${index}`]: value } as SwiftParseScope;
    });
    params.forEach((param, index) => {
      next = this.scopeWithLoopValue(next, param, values[index]);
    });
    return next;
  }

  private resolveStringMethod(
    value: string,
    method: string,
    args: string,
    scope: SwiftParseScope,
  ): unknown {
    const firstArg = this.splitTopLevel(args, ",")[0] ?? "";
    switch (method) {
      case "hasPrefix":
        return value.startsWith(this.evalExpression(firstArg, scope));
      case "hasSuffix":
        return value.endsWith(this.evalExpression(firstArg, scope));
      case "contains":
        return value.includes(this.evalExpression(firstArg, scope));
      case "uppercased":
        return value.toUpperCase();
      case "lowercased":
        return value.toLowerCase();
      case "split": {
        const separator =
          this.namedStringArg(args, "separator", scope) ??
          this.evalExpression(firstArg, scope);
        return value.split(separator);
      }
      default:
        return undefined;
    }
  }

  private formatSwiftValue(value: unknown, format: string, scope: SwiftParseScope): string {
    if (value === undefined || value === null) return "";
    if (Array.isArray(value) && format.includes("list")) {
      return this.formatSwiftList(value, format);
    }
    if (this.isSwiftDate(value)) {
      return this.formatSwiftDate(value, format);
    }
    if (this.isSwiftMeasurement(value)) {
      return this.formatSwiftMeasurement(value, format);
    }
    const numericValue = Number(value);
    if (Number.isFinite(numericValue)) {
      return this.formatSwiftNumber(numericValue, format, scope);
    }
    return String(value);
  }

  private formatSwiftList(value: unknown[], format: string): string {
    const type = format.includes(".or") || /type:\s*\.or/.test(format)
      ? "disjunction"
      : format.includes(".unit") || /type:\s*\.unit/.test(format)
        ? "unit"
        : "conjunction";
    const style = format.includes(".narrow") || /width:\s*\.narrow/.test(format)
      ? "narrow"
      : format.includes(".short") || /width:\s*\.short/.test(format)
        ? "short"
        : "long";
    return new Intl.ListFormat("en-US", { type, style }).format(
      value.map((entry) => (entry === undefined || entry === null ? "" : String(entry))),
    );
  }

  private formatSwiftNumber(value: number, format: string, scope: SwiftParseScope): string {
    const source = format.trim();
    if (source.includes("byteCount")) {
      return this.formatSwiftByteCount(value, source);
    }
    if (source.includes("currency")) {
      const call = this.readCall(source.replace(/^\./, ""));
      const currency =
        call?.name === "currency"
          ? (this.namedStringArg(call.args, "code", scope) ?? "USD")
          : "USD";
      return new Intl.NumberFormat("en-US", {
        style: "currency",
        currency,
      }).format(value);
    }
    if (source.includes("percent")) {
      return new Intl.NumberFormat("en-US", {
        style: "percent",
        maximumFractionDigits: 1,
      }).format(value);
    }
    if (source.includes("compactName") || source.includes("compact")) {
      return new Intl.NumberFormat("en-US", {
        notation: "compact",
        maximumFractionDigits: 1,
      }).format(value);
    }
    return new Intl.NumberFormat("en-US", {
      maximumFractionDigits: 3,
    }).format(value);
  }

  private formatSwiftByteCount(value: number, format: string): string {
    const binary = /\.memory\b|\.binary\b|countStyle:\s*\.binary/.test(format);
    const base = binary ? 1024 : 1000;
    const units = binary
      ? ["bytes", "KiB", "MiB", "GiB", "TiB"]
      : ["bytes", "KB", "MB", "GB", "TB"];
    const sign = value < 0 ? "-" : "";
    let size = Math.abs(value);
    let unitIndex = 0;
    while (size >= base && unitIndex < units.length - 1) {
      size /= base;
      unitIndex += 1;
    }
    if (unitIndex === 0) {
      const bytes = Math.trunc(size);
      return `${sign}${bytes} ${bytes === 1 ? "byte" : "bytes"}`;
    }
    const formatted = new Intl.NumberFormat("en-US", {
      maximumFractionDigits: 1,
    }).format(size);
    return `${sign}${formatted} ${units[unitIndex]}`;
  }

  private evalMeasurementInitializer(
    args: string,
    scope: SwiftParseScope,
  ): SwiftMeasurementValue | undefined {
    const value = this.toNumber(this.namedArg(args, "value"), scope);
    const unitArg = this.namedArg(args, "unit");
    if (value === undefined || unitArg === undefined) return undefined;
    return {
      __swiftMeasurement: true,
      value,
      unit: this.swiftMeasurementUnitToken(unitArg),
    };
  }

  private isSwiftMeasurement(value: unknown): value is SwiftMeasurementValue {
    return (
      typeof value === "object" &&
      value !== null &&
      (value as SwiftMeasurementValue).__swiftMeasurement === true &&
      typeof (value as SwiftMeasurementValue).value === "number" &&
      typeof (value as SwiftMeasurementValue).unit === "string"
    );
  }

  private isSwiftAngle(value: unknown): value is SwiftAngleValue {
    return (
      typeof value === "object" &&
      value !== null &&
      (value as SwiftAngleValue).__swiftAngle === true &&
      typeof (value as SwiftAngleValue).degrees === "number"
    );
  }

  private isSwiftUnitPoint(value: unknown): value is SwiftUnitPointValue {
    return (
      typeof value === "object" &&
      value !== null &&
      (value as SwiftUnitPointValue).__swiftUnitPoint === true &&
      typeof (value as SwiftUnitPointValue).x === "number" &&
      typeof (value as SwiftUnitPointValue).y === "number"
    );
  }

  private isSwiftPoint(value: unknown): value is SwiftPointValue {
    return (
      typeof value === "object" &&
      value !== null &&
      (value as SwiftPointValue).__swiftPoint === true &&
      typeof (value as SwiftPointValue).x === "number" &&
      typeof (value as SwiftPointValue).y === "number"
    );
  }

  private isSwiftSize(value: unknown): value is SwiftSizeValue {
    return (
      typeof value === "object" &&
      value !== null &&
      (value as SwiftSizeValue).__swiftSize === true &&
      typeof (value as SwiftSizeValue).width === "number" &&
      typeof (value as SwiftSizeValue).height === "number"
    );
  }

  private isSwiftRect(value: unknown): value is SwiftRectValue {
    return (
      typeof value === "object" &&
      value !== null &&
      (value as SwiftRectValue).__swiftRect === true &&
      typeof (value as SwiftRectValue).x === "number" &&
      typeof (value as SwiftRectValue).y === "number" &&
      typeof (value as SwiftRectValue).width === "number" &&
      typeof (value as SwiftRectValue).height === "number"
    );
  }

  private swiftGeometryDisplayValue(value: unknown): string | undefined {
    if (this.isSwiftAngle(value)) return String(value.degrees);
    if (this.isSwiftUnitPoint(value)) return value.token ?? `${value.x},${value.y}`;
    if (this.isSwiftPoint(value)) return `${value.x},${value.y}`;
    if (this.isSwiftSize(value)) return `${value.width}x${value.height}`;
    if (this.isSwiftRect(value)) return `${value.x},${value.y},${value.width}x${value.height}`;
    return undefined;
  }

  private swiftMeasurementUnitToken(expression: string): string {
    return expression
      .trim()
      .replace(/^\./, "")
      .replace(/^(?:UnitLength|UnitDuration|UnitInformationStorage|UnitMass|UnitTemperature)\./, "");
  }

  private formatSwiftMeasurement(measurement: SwiftMeasurementValue, format: string): string {
    const unit = this.swiftMeasurementUnitLabel(
      measurement.unit,
      format.includes(".wide") || /width:\s*\.wide/.test(format),
      measurement.value,
    );
    const value = new Intl.NumberFormat("en-US", {
      maximumFractionDigits: 1,
    }).format(measurement.value);
    return `${value} ${unit}`;
  }

  private swiftMeasurementUnitLabel(unit: string, wide: boolean, value: number): string {
    const singular = Math.abs(value) === 1;
    const labels: Record<string, { abbreviated: string; singular: string; plural: string }> = {
      bytes: { abbreviated: "bytes", singular: "byte", plural: "bytes" },
      kilobytes: { abbreviated: "KB", singular: "kilobyte", plural: "kilobytes" },
      megabytes: { abbreviated: "MB", singular: "megabyte", plural: "megabytes" },
      gigabytes: { abbreviated: "GB", singular: "gigabyte", plural: "gigabytes" },
      seconds: { abbreviated: "s", singular: "second", plural: "seconds" },
      minutes: { abbreviated: "min", singular: "minute", plural: "minutes" },
      hours: { abbreviated: "hr", singular: "hour", plural: "hours" },
      meters: { abbreviated: "m", singular: "meter", plural: "meters" },
      kilometers: { abbreviated: "km", singular: "kilometer", plural: "kilometers" },
      miles: { abbreviated: "mi", singular: "mile", plural: "miles" },
      feet: { abbreviated: "ft", singular: "foot", plural: "feet" },
      grams: { abbreviated: "g", singular: "gram", plural: "grams" },
      kilograms: { abbreviated: "kg", singular: "kilogram", plural: "kilograms" },
      celsius: { abbreviated: "C", singular: "degree Celsius", plural: "degrees Celsius" },
      fahrenheit: { abbreviated: "F", singular: "degree Fahrenheit", plural: "degrees Fahrenheit" },
    };
    const label = labels[unit] ?? {
      abbreviated: unit,
      singular: unit,
      plural: unit,
    };
    return wide ? (singular ? label.singular : label.plural) : label.abbreviated;
  }

  private evalDateInitializer(
    args: string,
    scope: SwiftParseScope,
  ): SwiftDateValue | undefined {
    if (args.trim() === "") {
      return { __swiftDate: true, epochMs: Date.now() };
    }
    const unixSeconds = this.toNumber(this.namedArg(args, "timeIntervalSince1970"), scope);
    if (unixSeconds !== undefined) {
      return { __swiftDate: true, epochMs: unixSeconds * 1000 };
    }
    const referenceSeconds = this.toNumber(this.namedArg(args, "timeIntervalSinceReferenceDate"), scope);
    if (referenceSeconds !== undefined) {
      return { __swiftDate: true, epochMs: (978_307_200 + referenceSeconds) * 1000 };
    }
    return undefined;
  }

  private isSwiftDate(value: unknown): value is SwiftDateValue {
    return (
      typeof value === "object" &&
      value !== null &&
      (value as SwiftDateValue).__swiftDate === true &&
      typeof (value as SwiftDateValue).epochMs === "number"
    );
  }

  private evalDateInterval(
    expression: string,
    scope: SwiftParseScope,
  ): SwiftDateIntervalValue | undefined {
    const trimmed = expression.trim();
    const dateIntervalCall = this.readCall(trimmed);
    if (dateIntervalCall?.name === "DateInterval" && dateIntervalCall.end === trimmed.length) {
      const start = this.evalValue(this.namedArg(dateIntervalCall.args, "start") ?? "", scope);
      const end = this.evalValue(this.namedArg(dateIntervalCall.args, "end") ?? "", scope);
      return this.isSwiftDate(start) && this.isSwiftDate(end) ? { start, end } : undefined;
    }
    const delimiter =
      this.topLevelIndexOf(trimmed, "...") >= 0
        ? "..."
        : this.topLevelIndexOf(trimmed, "..<") >= 0
          ? "..<"
          : undefined;
    if (delimiter === undefined) return undefined;
    const [startExpression, endExpression] = this.splitTopLevel(trimmed, delimiter);
    const start = this.evalValue(startExpression ?? "", scope);
    const end = this.evalValue(endExpression ?? "", scope);
    return this.isSwiftDate(start) && this.isSwiftDate(end) ? { start, end } : undefined;
  }

  private formatSwiftTextDateStyle(value: SwiftDateValue, style: string): string {
    if (style === "time") return this.formatSwiftDate(value, ".time");
    if (style === "relative" || style === "offset") {
      return this.formatSwiftRelativeDuration(value.epochMs - Date.now());
    }
    if (style === "timer") {
      return this.formatSwiftDuration(value.epochMs - Date.now());
    }
    return this.formatSwiftDate(value, ".date");
  }

  private formatSwiftDate(value: SwiftDateValue, format: string): string {
    const source = format.trim();
    const date = new Date(value.epochMs);
    if (Number.isNaN(date.getTime())) return "";
    const wantsTime = /(?:\.time\b|hour|minute|second)/.test(source);
    const wantsDate =
      !wantsTime || /(?:\.date\b|year|month|day|weekday)/.test(source);
    if (wantsDate && wantsTime) {
      return `${this.formatSwiftDatePart(date)}, ${this.formatSwiftTimePart(date)}`;
    }
    if (wantsTime) return this.formatSwiftTimePart(date);
    return this.formatSwiftDatePart(date);
  }

  private formatSwiftDatePart(date: Date): string {
    return new Intl.DateTimeFormat("en-US", {
      timeZone: "UTC",
      year: "numeric",
      month: "short",
      day: "numeric",
    }).format(date);
  }

  private formatSwiftTimePart(date: Date): string {
    return new Intl.DateTimeFormat("en-US", {
      timeZone: "UTC",
      hour: "numeric",
      minute: "2-digit",
    }).format(date);
  }

  private formatSwiftTimerInterval(
    interval: SwiftDateIntervalValue,
    countsDown: boolean,
  ): string {
    const delta = countsDown
      ? interval.end.epochMs - interval.start.epochMs
      : interval.start.epochMs - interval.end.epochMs;
    return this.formatSwiftDuration(delta);
  }

  private formatSwiftDuration(deltaMs: number): string {
    const totalSeconds = Math.max(0, Math.round(Math.abs(deltaMs) / 1000));
    const hours = Math.floor(totalSeconds / 3600);
    const minutes = Math.floor((totalSeconds % 3600) / 60);
    const seconds = totalSeconds % 60;
    if (hours > 0) {
      return `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
    }
    return `${minutes}:${String(seconds).padStart(2, "0")}`;
  }

  private formatSwiftRelativeDuration(deltaMs: number): string {
    const future = deltaMs >= 0;
    const totalSeconds = Math.max(0, Math.round(Math.abs(deltaMs) / 1000));
    const units: Array<[label: string, seconds: number]> = [
      ["day", 86_400],
      ["hour", 3_600],
      ["minute", 60],
      ["second", 1],
    ];
    const [unit, unitSeconds] =
      units.find(([, seconds]) => totalSeconds >= seconds) ?? units.at(-1)!;
    const value = Math.max(1, Math.round(totalSeconds / unitSeconds));
    const label = `${value} ${unit}${value === 1 ? "" : "s"}`;
    return future ? `in ${label}` : `${label} ago`;
  }

  private evalBuiltinFunction(
    name: string,
    args: string,
    scope: SwiftParseScope,
  ): unknown {
    if (name === "String") {
      const formatted = this.evalStringFormatInitializer(args, scope);
      if (formatted !== undefined) return formatted;
    }
    if (name === "Dictionary") {
      return this.evalDictionaryInitializer(args, scope);
    }
    if (name === "Measurement") {
      return this.evalMeasurementInitializer(args, scope);
    }
    if (name === "Date") {
      return this.evalDateInitializer(args, scope);
    }
    if (name === "GridItem") {
      return this.evalGridItemInitializer(args, scope);
    }
    if (name === "Angle") {
      return this.evalAngleInitializer(args, scope);
    }
    if (name === "UnitPoint") {
      return this.evalUnitPointInitializer(args, scope);
    }
    if (name === "CGPoint") {
      return this.evalPointInitializer(args, scope);
    }
    if (name === "CGSize") {
      return this.evalSizeInitializer(args, scope);
    }
    if (name === "CGRect") {
      return this.evalRectInitializer(args, scope);
    }
    const values = this.splitTopLevel(args, ",").map((arg) => this.evalValue(arg, scope));
    switch (name) {
      case "min": {
        const numbers = values.map(Number).filter(Number.isFinite);
        return numbers.length === 0 ? undefined : Math.min(...numbers);
      }
      case "max": {
        const numbers = values.map(Number).filter(Number.isFinite);
        return numbers.length === 0 ? undefined : Math.max(...numbers);
      }
      case "abs": {
        const value = Number(values[0]);
        return Number.isFinite(value) ? Math.abs(value) : undefined;
      }
      case "Int": {
        const value = Number(values[0]);
        return Number.isFinite(value) ? Math.trunc(value) : undefined;
      }
      case "Double": {
        const value = Number(values[0]);
        return Number.isFinite(value) ? value : undefined;
      }
      case "String":
        return values[0] === undefined || values[0] === null ? "" : String(values[0]);
      default:
        return undefined;
    }
  }

  private evalDottedSwiftGeometryLiteral(
    expression: string,
    scope: SwiftParseScope,
  ): unknown {
    const match = expression.match(
      /^(Angle|UnitPoint|CGPoint|CGSize|CGRect)\.([A-Za-z_][A-Za-z0-9_]*)(?:\(([\s\S]*)\))?$/,
    );
    if (!match) return undefined;
    const typeName = match[1] ?? "";
    const member = match[2] ?? "";
    const args = match[3] ?? "";
    if (typeName === "Angle") {
      if (member === "degrees") return this.swiftAngleFromDegrees(this.toNumber(args, scope));
      if (member === "radians") return this.swiftAngleFromRadians(this.toNumber(args, scope));
    }
    if (typeName === "UnitPoint") {
      return this.swiftUnitPointFromToken(member);
    }
    if (member === "zero") {
      if (typeName === "CGPoint") return { __swiftPoint: true, x: 0, y: 0 };
      if (typeName === "CGSize") return { __swiftSize: true, width: 0, height: 0 };
      if (typeName === "CGRect") return { __swiftRect: true, x: 0, y: 0, width: 0, height: 0 };
    }
    return undefined;
  }

  private evalAngleInitializer(
    args: string,
    scope: SwiftParseScope,
  ): SwiftAngleValue | undefined {
    return (
      this.swiftAngleFromDegrees(this.toNumber(this.namedArg(args, "degrees"), scope)) ??
      this.swiftAngleFromRadians(this.toNumber(this.namedArg(args, "radians"), scope)) ??
      this.swiftAngleFromDegrees(this.toNumber(this.firstPositionalExpression(args), scope))
    );
  }

  private swiftAngleFromDegrees(value: number | undefined): SwiftAngleValue | undefined {
    return value === undefined ? undefined : { __swiftAngle: true, degrees: value };
  }

  private swiftAngleFromRadians(value: number | undefined): SwiftAngleValue | undefined {
    return value === undefined
      ? undefined
      : { __swiftAngle: true, degrees: value * (180 / Math.PI) };
  }

  private evalUnitPointInitializer(
    args: string,
    scope: SwiftParseScope,
  ): SwiftUnitPointValue | undefined {
    const x = this.toNumber(this.namedArg(args, "x"), scope);
    const y = this.toNumber(this.namedArg(args, "y"), scope);
    return x === undefined || y === undefined
      ? undefined
      : {
          __swiftUnitPoint: true,
          x,
          y,
          token: this.swiftUnitPointTokenFromCoordinates(x, y),
        };
  }

  private swiftUnitPointFromToken(token: string): SwiftUnitPointValue | undefined {
    const coordinates: Record<string, [number, number]> = {
      center: [0.5, 0.5],
      top: [0.5, 0],
      bottom: [0.5, 1],
      leading: [0, 0.5],
      trailing: [1, 0.5],
      topLeading: [0, 0],
      topTrailing: [1, 0],
      bottomLeading: [0, 1],
      bottomTrailing: [1, 1],
    };
    const point = coordinates[token];
    return point === undefined
      ? undefined
      : { __swiftUnitPoint: true, x: point[0], y: point[1], token };
  }

  private swiftUnitPointTokenFromCoordinates(x: number, y: number): string | undefined {
    const pairs: Array<[string, number, number]> = [
      ["center", 0.5, 0.5],
      ["top", 0.5, 0],
      ["bottom", 0.5, 1],
      ["leading", 0, 0.5],
      ["trailing", 1, 0.5],
      ["topLeading", 0, 0],
      ["topTrailing", 1, 0],
      ["bottomLeading", 0, 1],
      ["bottomTrailing", 1, 1],
    ];
    return pairs.find(([, pointX, pointY]) => pointX === x && pointY === y)?.[0];
  }

  private evalPointInitializer(
    args: string,
    scope: SwiftParseScope,
  ): SwiftPointValue | undefined {
    const x = this.toNumber(this.namedArg(args, "x"), scope) ?? 0;
    const y = this.toNumber(this.namedArg(args, "y"), scope) ?? 0;
    return { __swiftPoint: true, x, y };
  }

  private evalSizeInitializer(
    args: string,
    scope: SwiftParseScope,
  ): SwiftSizeValue | undefined {
    const width = this.toNumber(this.namedArg(args, "width"), scope) ?? 0;
    const height = this.toNumber(this.namedArg(args, "height"), scope) ?? 0;
    return { __swiftSize: true, width, height };
  }

  private evalRectInitializer(
    args: string,
    scope: SwiftParseScope,
  ): SwiftRectValue | undefined {
    const origin = this.evalValue(this.namedArg(args, "origin") ?? "", scope);
    const size = this.evalValue(this.namedArg(args, "size") ?? "", scope);
    const x = this.isSwiftPoint(origin)
      ? origin.x
      : (this.toNumber(this.namedArg(args, "x"), scope) ?? 0);
    const y = this.isSwiftPoint(origin)
      ? origin.y
      : (this.toNumber(this.namedArg(args, "y"), scope) ?? 0);
    const width = this.isSwiftSize(size)
      ? size.width
      : (this.toNumber(this.namedArg(args, "width"), scope) ?? 0);
    const height = this.isSwiftSize(size)
      ? size.height
      : (this.toNumber(this.namedArg(args, "height"), scope) ?? 0);
    return { __swiftRect: true, x, y, width, height };
  }

  private evalGridItemInitializer(
    args: string,
    scope: SwiftParseScope,
  ): SwiftGridItemValue | undefined {
    const sizeExpression = this.firstPositionalExpression(args) ?? ".flexible()";
    const size = this.swiftGridItemSize(sizeExpression, scope);
    if (size === undefined) return undefined;
    return {
      __swiftGridItem: true,
      ...size,
      spacing: this.toNumber(this.namedArg(args, "spacing"), scope),
      alignment: this.swiftAlignmentToken(this.namedArg(args, "alignment"), scope),
    };
  }

  private swiftGridItemSize(
    expression: string,
    scope: SwiftParseScope,
  ):
    | Pick<SwiftGridItemValue, "size" | "minimum" | "maximum">
    | undefined {
    const trimmed = expression.trim();
    const match = trimmed.match(
      /^(?:\.|GridItem\.Size\.)?(fixed|flexible|adaptive)\s*(?:\(([\s\S]*)\))?$/,
    );
    if (!match) return undefined;
    const size = match[1] as SwiftGridItemValue["size"];
    const args = match[2] ?? "";
    if (size === "fixed") {
      return {
        size,
        minimum: this.toNumber(this.firstPositionalExpression(args), scope),
      };
    }
    if (size === "adaptive") {
      return {
        size,
        minimum:
          this.toNumber(this.namedArg(args, "minimum"), scope) ??
          this.toNumber(this.firstPositionalExpression(args), scope),
        maximum: this.toNumber(this.namedArg(args, "maximum"), scope),
      };
    }
    return {
      size,
      minimum: this.toNumber(this.namedArg(args, "minimum"), scope),
      maximum: this.toNumber(this.namedArg(args, "maximum"), scope),
    };
  }

  private evalStringFormatInitializer(
    args: string,
    scope: SwiftParseScope,
  ): string | undefined {
    const formatArg = this.namedArg(args, "format");
    if (formatArg === undefined) return undefined;
    const format = this.evalExpression(formatArg, scope);
    const positionalValues = this.splitTopLevel(args, ",")
      .filter((arg) => this.topLevelIndexOf(arg, ":") < 0)
      .map((arg) => this.evalValue(arg, scope));
    const argumentList = this.namedArg(args, "arguments");
    const listedValues =
      argumentList === undefined ? [] : this.evalValue(argumentList, scope);
    const values = Array.isArray(listedValues)
      ? [...positionalValues, ...listedValues]
      : [...positionalValues];
    return this.formatSwiftString(format, values);
  }

  private evalDictionaryInitializer(
    args: string,
    scope: SwiftParseScope,
  ): Record<string, unknown[]> | undefined {
    const groupingExpression =
      this.namedArg(args, "grouping") ?? this.firstPositionalExpression(args);
    if (groupingExpression === undefined) return undefined;
    const groupedValue = this.evalValue(groupingExpression, scope);
    if (!Array.isArray(groupedValue)) return undefined;
    const byExpression = this.namedArg(args, "by");
    if (byExpression === undefined) return undefined;
    const projection = this.arrayProjection(byExpression, scope);
    if (projection === undefined) return undefined;
    const record: Record<string, unknown[]> = {};
    groupedValue.forEach((entry, index) => {
      const key = projection(entry, index);
      const stringKey = key === undefined || key === null ? "" : String(key);
      record[stringKey] = [...(record[stringKey] ?? []), entry];
    });
    return record;
  }

  private formatSwiftString(format: string, values: unknown[]): string {
    let output = "";
    let formatIndex = 0;
    let valueIndex = 0;
    while (formatIndex < format.length) {
      const percentIndex = format.indexOf("%", formatIndex);
      if (percentIndex < 0) {
        output += format.slice(formatIndex);
        break;
      }
      output += format.slice(formatIndex, percentIndex);
      if (format[percentIndex + 1] === "%") {
        output += "%";
        formatIndex = percentIndex + 2;
        continue;
      }
      const specifier = format.slice(percentIndex).match(
        /^%([-+0 #]*)(\d+)?(?:\.(\d+))?([@sdifuoxXfFeEgG])/,
      );
      if (specifier === null) {
        output += "%";
        formatIndex = percentIndex + 1;
        continue;
      }
      output += this.formatSwiftSpecifier(
        values[valueIndex],
        specifier[4] ?? "@",
        specifier[1] ?? "",
        specifier[2],
        specifier[3],
      );
      valueIndex += 1;
      formatIndex = percentIndex + specifier[0].length;
    }
    return output;
  }

  private formatSwiftSpecifier(
    value: unknown,
    specifier: string,
    flags: string,
    width: string | undefined,
    precision: string | undefined,
  ): string {
    const numberValue = Number(value);
    const precisionValue = precision === undefined ? undefined : Number(precision);
    let formatted: string;
    switch (specifier) {
      case "d":
      case "i":
      case "u":
        formatted = Number.isFinite(numberValue) ? String(Math.trunc(numberValue)) : "0";
        break;
      case "o":
        formatted = Number.isFinite(numberValue) ? Math.trunc(numberValue).toString(8) : "0";
        break;
      case "x":
      case "X":
        formatted = Number.isFinite(numberValue) ? Math.trunc(numberValue).toString(16) : "0";
        formatted = specifier === "X" ? formatted.toUpperCase() : formatted;
        break;
      case "f":
      case "F":
        formatted = Number.isFinite(numberValue)
          ? numberValue.toFixed(Number.isFinite(precisionValue) ? precisionValue : 6)
          : "nan";
        formatted = specifier === "F" ? formatted.toUpperCase() : formatted;
        break;
      case "e":
      case "E":
        formatted = Number.isFinite(numberValue)
          ? numberValue.toExponential(Number.isFinite(precisionValue) ? precisionValue : 6)
          : "nan";
        formatted = specifier === "E" ? formatted.toUpperCase() : formatted;
        break;
      case "g":
      case "G":
        formatted = Number.isFinite(numberValue)
          ? numberValue.toPrecision(Number.isFinite(precisionValue) ? precisionValue : 6)
          : "nan";
        formatted = specifier === "G" ? formatted.toUpperCase() : formatted;
        break;
      case "@":
      case "s":
      default:
        formatted = value === undefined || value === null ? "" : String(value);
        break;
    }
    const widthValue = width === undefined ? undefined : Number(width);
    if (widthValue === undefined || !Number.isFinite(widthValue) || formatted.length >= widthValue) {
      return formatted;
    }
    const padChar = flags.includes("0") && !flags.includes("-") ? "0" : " ";
    const padding = padChar.repeat(widthValue - formatted.length);
    return flags.includes("-") ? `${formatted}${padding}` : `${padding}${formatted}`;
  }

  private interpolateSwiftString(value: string, scope: SwiftParseScope): string {
    let output = "";
    let index = 0;
    while (index < value.length) {
      if (value[index] === "\\" && value[index + 1] === "(") {
        const balanced = this.readBalanced(value, index + 1, "(", ")");
        if (balanced !== null) {
          output += this.evalExpression(balanced.content, scope);
          index = balanced.end;
          continue;
        }
      }
      output += value[index] ?? "";
      index += 1;
    }
    return output;
  }

  private evalSequence(sequence: string, scope: SwiftParseScope): unknown[] | null {
    const trimmed = sequence.trim();
    const halfOpen = trimmed.match(/^(.+?)\s*\.\.<\s*(.+)$/);
    const closed = trimmed.match(/^(.+?)\s*\.\.\.\s*(.+)$/);
    const range = halfOpen ?? closed;
    if (range) {
      const start = this.toNumber(range[1], scope);
      const end = this.toNumber(range[2], scope);
      if (start === undefined || end === undefined) return null;
      const limit = closed ? end + 1 : end;
      const values: number[] = [];
      for (let value = start; value < limit && values.length < 250; value += 1) {
        values.push(value);
      }
      return values;
    }
    const resolved = this.resolvePath(trimmed, scope);
    if (Array.isArray(resolved)) return resolved;
    return null;
  }

  private scopeWithLoopValue(
    scope: SwiftParseScope,
    param: string,
    value: unknown,
  ): SwiftParseScope {
    const next = { ...scope, [param]: value } as SwiftParseScope;
    if (this.isWorkspacePreview(value)) {
      return { ...next, workspace: value };
    }
    return next;
  }

  private scopeWithLoopValues(
    scope: SwiftParseScope,
    params: string[],
    value: unknown,
  ): SwiftParseScope {
    if (params.length <= 1) {
      return this.scopeWithLoopValue(scope, params[0] ?? "item", value);
    }
    const pair =
      typeof value === "object" && value !== null
        ? (value as { offset?: unknown; element?: unknown })
        : {};
    let next = { ...scope } as SwiftParseScope;
    next = this.scopeWithLoopValue(next, params[0] ?? "offset", pair.offset);
    next = this.scopeWithLoopValue(next, params[1] ?? "element", pair.element);
    return next;
  }

  private scopeWithForEachValues(
    scope: SwiftParseScope,
    params: string[],
    value: unknown,
    bindingCollectionKey: string | undefined,
    valueIndex: number,
  ): SwiftParseScope {
    const next = this.scopeWithLoopValues(scope, params, value);
    if (bindingCollectionKey === undefined || params.length > 1) return next;
    const param = params[0] ?? "item";
    return {
      ...next,
      __bindingKeys: {
        ...(next.__bindingKeys ?? {}),
        [param]: `${bindingCollectionKey}[${valueIndex}]`,
      },
    } as SwiftParseScope;
  }

  private isWorkspacePreview(value: unknown): value is WorkspacePreview {
    return (
      typeof value === "object" &&
      value !== null &&
      typeof (value as WorkspacePreview).id === "string" &&
      typeof (value as WorkspacePreview).title === "string"
    );
  }

  private firstPositionalArg(args: string, scope: SwiftParseScope): string | undefined {
    const first = this.splitTopLevel(args, ",").find(
      (arg) => this.topLevelIndexOf(arg, ":") < 0,
    );
    return first === undefined ? undefined : this.evalExpression(first, scope);
  }

  private firstPositionalExpression(args: string): string | undefined {
    return this.splitTopLevel(args, ",").find((arg) => this.topLevelIndexOf(arg, ":") < 0)?.trim();
  }

  private swiftListIdToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const token = this.evalExpression(expression, scope)
      .replace(/^\\?\./, "")
      .trim();
    return token === "" ? undefined : token;
  }

  private presentationDetentsValue(args: string, scope: SwiftParseScope): string | undefined {
    const trimmed = args.trim();
    if (!trimmed) return undefined;
    const list = trimmed.startsWith("[") && trimmed.endsWith("]") ? trimmed.slice(1, -1) : trimmed;
    const detents = this.splitTopLevel(list, ",")
      .map((part) => part.trim())
      .filter(Boolean)
      .map((part) =>
        String(this.evalExpression(part, scope) ?? part)
          .replace(/^(?:\.|PresentationDetent\.)/, "")
          .trim(),
      )
      .filter(Boolean);
    return detents.length === 0 ? undefined : detents.join(",");
  }

  private accessibilityTokenValue(args: string, scope: SwiftParseScope): string | undefined {
    const trimmed = args.trim();
    if (!trimmed) return undefined;
    const list = trimmed.startsWith("[") && trimmed.endsWith("]") ? trimmed.slice(1, -1) : trimmed;
    const tokens = this.splitTopLevel(list, ",")
      .map((part) => part.trim())
      .filter(Boolean)
      .map((part) =>
        String(this.evalExpression(part, scope) ?? part)
          .replace(/^(?:\.|AccessibilityTraits\.)/, "")
          .trim(),
      )
      .filter(Boolean);
    return tokens.length === 0 ? undefined : tokens.join(",");
  }

  private accessibilityElementChildrenValue(
    args: string,
    scope: SwiftParseScope,
  ): string | undefined {
    const children = this.namedArg(args, "children") ?? this.firstPositionalArg(args, scope);
    return children
      ?.replace(/^(?:\.|AccessibilityChildBehavior\.)/, "")
      .trim();
  }

  private accessibilityActionValue(args: string, scope: SwiftParseScope): string {
    const expression = this.namedArg(args, "named") ?? this.firstPositionalExpression(args);
    if (expression === undefined || expression.trim() === "") return "default";
    const trimmed = expression.trim();
    const textCall = this.readCall(trimmed);
    if (textCall?.name === "Text" && textCall.end === trimmed.length) {
      return this.firstPositionalArg(textCall.args, scope) ?? "named";
    }
    const value = this.evalExpression(trimmed, scope).replace(
      /^(?:\.|AccessibilityActionKind\.)/,
      "",
    );
    return value.trim() === "" ? "default" : value.trim();
  }

  private swiftKeyboardShortcutModifiers(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const trimmed = expression.trim();
    const source = trimmed.startsWith("[") && trimmed.endsWith("]")
      ? trimmed.slice(1, -1)
      : trimmed;
    const modifiers = new Set<string>();
    for (const part of this.splitTopLevel(source, ",")) {
      const token = this.evalExpression(part, scope)
        .replace(/^(?:\.|EventModifiers\.)/, "")
        .trim();
      switch (token) {
        case "command":
        case "cmd":
          modifiers.add("command");
          break;
        case "shift":
          modifiers.add("shift");
          break;
        case "option":
        case "alt":
          modifiers.add("option");
          break;
        case "control":
        case "ctrl":
          modifiers.add("control");
          break;
        default:
          break;
      }
    }
    return modifiers.size === 0 ? undefined : [...modifiers].join(",");
  }

  private namedArg(args: string | undefined, label: string): string | undefined {
    if (args === undefined) return undefined;
    for (const arg of this.splitTopLevel(args, ",")) {
      const colon = this.topLevelIndexOf(arg, ":");
      if (colon < 0) continue;
      if (arg.slice(0, colon).trim() === label) {
        return arg.slice(colon + 1).trim();
      }
    }
    return undefined;
  }

  private namedStringArg(
    args: string,
    label: string,
    scope: SwiftParseScope,
  ): string | undefined {
    const value = this.namedArg(args, label);
    return value === undefined ? undefined : this.evalExpression(value, scope);
  }

  private toNumber(expression: string | undefined, scope: SwiftParseScope): number | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const resolved = this.evalValue(expression.trim(), scope);
    const value = this.isSwiftAngle(resolved) ? resolved.degrees : Number(resolved);
    return Number.isFinite(value) ? value : undefined;
  }

  private swiftDegrees(args: string, scope: SwiftParseScope): number | undefined {
    const expression = this.firstPositionalExpression(args);
    if (expression !== undefined) {
      const value = this.evalValue(expression, scope);
      if (this.isSwiftAngle(value)) return value.degrees;
    }
    const degrees = args.match(/\.degrees\(([^)]+)\)/);
    if (degrees) return this.toNumber(degrees[1], scope);
    const radians = args.match(/\.radians\(([^)]+)\)/);
    if (radians) return this.swiftAngleFromRadians(this.toNumber(radians[1], scope))?.degrees;
    return this.toNumber(this.firstPositionalArg(args, scope), scope);
  }

  private swiftAxisTuple(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): { x: number; y: number; z: number } | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const trimmed = expression.trim();
    const source = trimmed.startsWith("(") && trimmed.endsWith(")")
      ? trimmed.slice(1, -1)
      : trimmed;
    const parts = this.splitTopLevel(source, ",");
    return {
      x: this.toNumber(this.namedArg(source, "x") ?? parts[0], scope) ?? 0,
      y: this.toNumber(this.namedArg(source, "y") ?? parts[1], scope) ?? 0,
      z: this.toNumber(this.namedArg(source, "z") ?? parts[2], scope) ?? 0,
    };
  }

  private swiftUnitPointToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const value = this.evalValue(expression, scope);
    if (this.isSwiftUnitPoint(value)) return value.token;
    const token = this.evalExpression(expression, scope)
      .replace(/^(?:\.|UnitPoint\.)/, "")
      .trim();
    switch (token) {
      case "center":
      case "top":
      case "bottom":
      case "leading":
      case "trailing":
      case "topLeading":
      case "topTrailing":
      case "bottomLeading":
      case "bottomTrailing":
        return token;
      default:
        return undefined;
    }
  }

  private swiftPointValue(
    expression: string | undefined,
    args: string,
    scope: SwiftParseScope,
  ): { x?: number; y?: number } {
    const source = expression?.trim();
    const value = source === undefined ? undefined : this.evalValue(source, scope);
    if (this.isSwiftPoint(value) || this.isSwiftUnitPoint(value)) {
      return { x: value.x, y: value.y };
    }
    const call = source === undefined ? null : this.readCall(source);
    const callArgs =
      call !== null && (call.name === "CGPoint" || call.name === "init") && call.end === source?.length
        ? call.args
        : undefined;
    const x = this.toNumber(
      this.namedArg(callArgs, "x") ?? this.namedArg(args, "x"),
      scope,
    );
    const y = this.toNumber(
      this.namedArg(callArgs, "y") ?? this.namedArg(args, "y"),
      scope,
    );
    return {
      ...(x !== undefined ? { x } : {}),
      ...(y !== undefined ? { y } : {}),
    };
  }

  private swiftRectValue(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): SwiftRectValue | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const value = this.evalValue(expression, scope);
    return this.isSwiftRect(value) ? value : undefined;
  }

  private swiftCoordinateSpaceToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const trimmed = expression.trim();
    const source = trimmed.startsWith(".") ? trimmed.slice(1) : trimmed;
    const namedCall = this.readCall(source);
    if (namedCall?.name === "named" && namedCall.end === source.length) {
      return this.firstPositionalArg(namedCall.args, scope);
    }
    const token = this.evalExpression(trimmed, scope)
      .replace(/^(?:\.|CoordinateSpace\.)/, "")
      .trim();
    return token === "" ? undefined : token;
  }

  private swiftAlignmentGuideToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const token = this.evalExpression(expression, scope)
      .replace(/^(?:\.|HorizontalAlignment\.|VerticalAlignment\.)/, "")
      .trim();
    switch (token) {
      case "leading":
      case "center":
      case "trailing":
      case "top":
      case "bottom":
      case "firstTextBaseline":
      case "lastTextBaseline":
        return token;
      default:
        return token === "" ? undefined : token;
    }
  }

  private alignmentGuideOffsetValue(
    trailingBody: string,
    scope: SwiftParseScope,
  ): string | undefined {
    const body = this.splitClosureParameter(trailingBody).body.trim();
    if (body === "") return undefined;
    const returnMatch = body.match(/^return\s+([\s\S]+)$/);
    const expression = returnMatch?.[1] ?? body;
    const value = this.toNumber(expression, scope);
    return value === undefined ? undefined : String(value);
  }

  private swiftEdgeInsetsValue(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): Pick<
    CustomSidebarSwiftModifier,
    "capInsetTop" | "capInsetLeading" | "capInsetBottom" | "capInsetTrailing"
  > {
    if (expression === undefined || expression.trim() === "") return {};
    const trimmed = expression.trim();
    const call = this.readCall(trimmed);
    const source = call?.name === "EdgeInsets" || call?.name === ".init" ? call.args : trimmed;
    const top = this.toNumber(this.namedArg(source, "top"), scope);
    const leading = this.toNumber(this.namedArg(source, "leading"), scope);
    const bottom = this.toNumber(this.namedArg(source, "bottom"), scope);
    const trailing = this.toNumber(this.namedArg(source, "trailing"), scope);
    return {
      ...(top !== undefined ? { capInsetTop: top } : {}),
      ...(leading !== undefined ? { capInsetLeading: leading } : {}),
      ...(bottom !== undefined ? { capInsetBottom: bottom } : {}),
      ...(trailing !== undefined ? { capInsetTrailing: trailing } : {}),
    };
  }

  private swiftPaddingInsetsValue(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): Pick<
    CustomSidebarSwiftModifier,
    "paddingTop" | "paddingLeading" | "paddingBottom" | "paddingTrailing"
  > {
    if (expression === undefined || expression.trim() === "") return {};
    const trimmed = expression.trim();
    const call = this.readCall(trimmed);
    if (call?.name !== "EdgeInsets" && call?.name !== ".init") return {};
    const top = this.toNumber(this.namedArg(call.args, "top"), scope);
    const leading = this.toNumber(this.namedArg(call.args, "leading"), scope);
    const bottom = this.toNumber(this.namedArg(call.args, "bottom"), scope);
    const trailing = this.toNumber(this.namedArg(call.args, "trailing"), scope);
    return {
      ...(top !== undefined ? { paddingTop: top } : {}),
      ...(leading !== undefined ? { paddingLeading: leading } : {}),
      ...(bottom !== undefined ? { paddingBottom: bottom } : {}),
      ...(trailing !== undefined ? { paddingTrailing: trailing } : {}),
    };
  }

  private swiftContentMarginPlacementToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const token = this.evalExpression(expression, scope)
      .replace(/^(?:\.|ContentMarginPlacement\.)/, "")
      .trim();
    switch (token) {
      case "automatic":
      case "scrollContent":
      case "scrollIndicators":
        return token;
      default:
        return undefined;
    }
  }

  private swiftResizableModeToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const token = this.evalExpression(expression, scope)
      .replace(/^(?:\.|Image\.ResizingMode\.)/, "")
      .trim();
    switch (token) {
      case "stretch":
      case "tile":
        return token;
      default:
        return undefined;
    }
  }

  private swiftColorSchemeToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const token = this.evalExpression(expression, scope)
      .replace(/^(?:\.|ColorScheme\.)/, "")
      .trim();
    return token === "dark" || token === "light" ? token : undefined;
  }

  private swiftLayoutDirectionToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const token = this.evalExpression(expression, scope)
      .replace(/^(?:\.|LayoutDirection\.)/, "")
      .trim();
    switch (token) {
      case "rightToLeft":
      case "rtl":
        return "rightToLeft";
      case "leftToRight":
      case "ltr":
        return "leftToRight";
      default:
        return undefined;
    }
  }

  private swiftEnvironmentKeyToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): "colorScheme" | "layoutDirection" | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const token = this.evalExpression(expression, scope)
      .replace(/^(?:\\?\.|EnvironmentValues\.)/, "")
      .trim();
    switch (token) {
      case "colorScheme":
      case "layoutDirection":
        return token;
      default:
        return undefined;
    }
  }

  private swiftEnvironmentReadKeyToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): "colorScheme" | "layoutDirection" | "locale" | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const token = this.evalExpression(expression, scope)
      .replace(/^(?:\\?\.|EnvironmentValues\.)/, "")
      .trim();
    switch (token) {
      case "colorScheme":
      case "layoutDirection":
      case "locale":
        return token;
      default:
        return undefined;
    }
  }

  private swiftEnvironmentReadValue(
    key: "colorScheme" | "layoutDirection" | "locale",
  ): unknown {
    switch (key) {
      case "colorScheme":
        return "light";
      case "layoutDirection":
        return "leftToRight";
      case "locale":
        return {
          identifier: "en-US",
          languageCode: "en",
        };
      default:
        return undefined;
    }
  }

  private swiftShapeToken(args: string): string | undefined {
    const token = args.match(/\b(Circle|Capsule|Rectangle|RoundedRectangle)\s*\(/);
    return token?.[1]?.replace(/^./, (char) => char.toLowerCase());
  }

  private swiftRoundedCornerStyleToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const token = this.evalExpression(expression, scope)
      .replace(/^(?:\.|RoundedCornerStyle\.)/, "")
      .trim();
    return token === "continuous" || token === "circular" ? token : undefined;
  }

  private swiftFillStyleMetadata(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): Pick<CustomSidebarSwiftModifier, "fillStyle" | "antialiased"> {
    if (expression === undefined || expression.trim() === "") return {};
    const trimmed = expression.trim();
    const call = this.readCall(trimmed);
    const source = call?.name === "FillStyle" || call?.name === ".init" ? call.args : trimmed;
    const eoFill = this.namedArg(source, "eoFill");
    const antialiased = this.namedArg(source, "antialiased");
    return {
      ...(eoFill !== undefined && this.evalCondition(eoFill, scope) === true
        ? { fillStyle: "eoFill" }
        : {}),
      ...(antialiased !== undefined
        ? { antialiased: this.evalCondition(antialiased, scope) }
        : {}),
    };
  }

  private swiftEdgeSet(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string[] | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const trimmed = expression.trim();
    const source = trimmed.startsWith("[") && trimmed.endsWith("]")
      ? trimmed.slice(1, -1)
      : trimmed;
    const edges = new Set<string>();
    for (const part of this.splitTopLevel(source, ",")) {
      const token = this.evalExpression(part, scope)
        .replace(/^(?:\.|Edge\.Set\.)/, "")
        .trim();
      switch (token) {
        case "all":
          edges.add("top");
          edges.add("trailing");
          edges.add("bottom");
          edges.add("leading");
          break;
        case "horizontal":
          edges.add("leading");
          edges.add("trailing");
          break;
        case "vertical":
          edges.add("top");
          edges.add("bottom");
          break;
        case "top":
        case "bottom":
        case "leading":
        case "trailing":
          edges.add(token);
          break;
        default:
          break;
      }
    }
    return edges.size === 0 ? undefined : [...edges];
  }

  private swiftAlignmentToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const token = this.evalExpression(expression, scope)
      .replace(/^(?:\.|Alignment\.)/, "")
      .trim();
    switch (token) {
      case "leading":
      case "trailing":
      case "center":
      case "top":
      case "bottom":
      case "topLeading":
      case "topTrailing":
      case "bottomLeading":
      case "bottomTrailing":
        return token;
      default:
        return undefined;
    }
  }

  private swiftPinnedViews(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const trimmed = expression.trim();
    const source = trimmed.startsWith("[") && trimmed.endsWith("]")
      ? trimmed.slice(1, -1)
      : trimmed;
    const pinned = new Set<string>();
    for (const part of this.splitTopLevel(source, ",")) {
      const token = this.evalExpression(part, scope)
        .replace(/^(?:\.|PinnedScrollableViews\.)/, "")
        .trim();
      switch (token) {
        case "sectionHeaders":
        case "sectionFooters":
          pinned.add(token);
          break;
        default:
          break;
      }
    }
    return pinned.size === 0 ? undefined : [...pinned].join(",");
  }

  private swiftGridItems(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): SwiftGridItemValue[] | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const value = this.evalValue(expression, scope);
    if (!Array.isArray(value)) return undefined;
    const items = value.filter(isSwiftGridItem);
    return items.length === 0 ? undefined : items;
  }

  private readViewStatement(
    body: string,
    index: number,
  ): { text: string; end: number } | null {
    const call = this.readViewCall(body.slice(index));
    if (call === null) return null;
    let end = index + call.end;
    if (
      call.name === "AsyncImage" ||
      call.name === "GroupBox" ||
      call.name === "DisclosureGroup" ||
      call.name === "ContentUnavailableView"
    ) {
      const sequence = this.readTrailingClosureSequence(body.slice(end));
      if (sequence.end > 0) {
        end += sequence.end;
      }
    } else {
      const trailing = this.readTrailingClosure(body.slice(end));
      if (trailing !== null) end += trailing.end;
    }
    while (true) {
      const next = this.skipWhitespaceAndSeparators(body, end);
      if (body[next] !== ".") break;
      const modifier = this.readModifierCall(body.slice(next + 1));
      if (modifier === null) break;
      end = next + 1 + modifier.end;
      const modifierTrailing = this.readTrailingClosure(body.slice(end));
      if (modifierTrailing !== null) end += modifierTrailing.end;
    }
    const next = this.skipWhitespaceAndSeparators(body, end);
    if (body[next] === "+") {
      const rhs = this.readViewStatement(body, next + 1);
      if (rhs !== null) end = rhs.end;
    }
    return { text: body.slice(index, end), end };
  }

  private readSimpleStatement(
    body: string,
    index: number,
  ): { text: string; end: number } {
    let depth = 0;
    let inString = false;
    for (let i = index; i < body.length; i += 1) {
      const char = body[i];
      if (char === '"' && body[i - 1] !== "\\") inString = !inString;
      if (inString) continue;
      if (char === "(" || char === "[" || char === "{") depth += 1;
      if (char === ")" || char === "]" || char === "}") depth -= 1;
      if (depth === 0 && (char === "\n" || char === ";")) {
        return { text: body.slice(index, i).trim(), end: i + 1 };
      }
    }
    return { text: body.slice(index).trim(), end: body.length };
  }

  private readCall(text: string): { name: string; args: string; end: number } | null {
    const match = text.match(/^\s*([A-Za-z_][A-Za-z0-9_]*)\s*\(/);
    if (!match) return null;
    const open = text.indexOf("(", match[0].length - 1);
    const balanced = this.readBalanced(text, open, "(", ")");
    if (balanced === null) return null;
    return { name: match[1] ?? "", args: balanced.content, end: balanced.end };
  }

  private readModifierCall(text: string): { name: string; args: string; end: number } | null {
    const call = this.readCall(text);
    if (call !== null) return call;
    const match = text.match(/^\s*([A-Za-z_][A-Za-z0-9_]*)\b/);
    if (!match) return null;
    const end = match[0].length;
    const next = this.skipWhitespaceAndSeparators(text, end);
    return text[next] === "{" ? { name: match[1] ?? "", args: "", end } : null;
  }

  private readViewCall(text: string): { name: string; args: string; end: number } | null {
    const call = this.readCall(text);
    if (call !== null) return call;
    const match = text.match(/^\s*([A-Za-z_][A-Za-z0-9_]*)\b/);
    if (!match) return null;
    const end = match[0].length;
    const next = this.skipWhitespaceAndSeparators(text, end);
    return text[next] === "{" ? { name: match[1] ?? "", args: "", end } : null;
  }

  private readTrailingClosure(text: string): { body: string; end: number } | null {
    const start = this.skipWhitespaceAndSeparators(text, 0);
    if (text[start] !== "{") return null;
    const balanced = this.readBalanced(text, start, "{", "}");
    return balanced === null ? null : { body: balanced.content, end: balanced.end };
  }

  private readTrailingClosureSequence(
    text: string,
  ): { first?: string; labeled: Record<string, string>; end: number } {
    const first = this.readTrailingClosure(text);
    if (first === null) return { labeled: {}, end: 0 };
    const labeled: Record<string, string> = {};
    let end = first.end;
    while (true) {
      const labelStart = this.skipWhitespaceAndSeparators(text, end);
      const label = text.slice(labelStart).match(/^([A-Za-z_][A-Za-z0-9_]*)\s*:/);
      if (label === null) break;
      const labelName = label[1] ?? "";
      if (
        labelName !== "content" &&
        labelName !== "placeholder" &&
        labelName !== "label" &&
        labelName !== "description" &&
        labelName !== "actions"
      ) {
        break;
      }
      const closure = this.readTrailingClosure(text.slice(labelStart + label[0].length));
      if (closure === null) break;
      labeled[labelName] = closure.body;
      end = labelStart + label[0].length + closure.end;
    }
    return { first: first.body, labeled, end };
  }

  private readBalanced(
    text: string,
    open: number,
    openChar: string,
    closeChar: string,
  ): { content: string; end: number } | null {
    let depth = 0;
    let inString = false;
    for (let i = open; i < text.length; i += 1) {
      const char = text[i];
      const prev = text[i - 1];
      if (char === '"' && prev !== "\\") inString = !inString;
      if (inString) continue;
      if (char === openChar) depth += 1;
      if (char === closeChar) {
        depth -= 1;
        if (depth === 0) return { content: text.slice(open + 1, i), end: i + 1 };
      }
    }
    return null;
  }

  private splitClosureParameter(body: string): { params: string[]; body: string } {
    const match = body.match(
      /^\s*(\(?\s*\$?[A-Za-z_][A-Za-z0-9_]*(?:\s*,\s*\$?[A-Za-z_][A-Za-z0-9_]*)*\s*\)?)\s+in\b/,
    );
    if (!match) return { params: [], body };
    const params = body
      .slice(0, match[0].lastIndexOf(" in"))
      .trim()
      .replace(/[()]/g, "")
      .split(",")
      .map((param) => param.trim().replace(/^\$/, ""))
      .filter(Boolean);
    return {
      params,
      body: body.slice(match[0].length),
    };
  }

  private escapeRegExp(value: string): string {
    return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  }

  private extractNamedClosure(args: string, label: string): string | null {
    const index = args.indexOf(`${label}:`);
    if (index < 0) return null;
    const open = args.indexOf("{", index);
    const block = open >= 0 ? this.readBalanced(args, open, "{", "}") : null;
    return block?.content ?? null;
  }

  private splitTernary(text: string): { condition: string; truthy: string; falsy: string } | null {
    const question = this.topLevelIndexOf(text, "?");
    if (question < 0) return null;
    const colon = this.topLevelIndexOf(text.slice(question + 1), ":");
    if (colon < 0) return null;
    return {
      condition: text.slice(0, question),
      truthy: text.slice(question + 1, question + 1 + colon),
      falsy: text.slice(question + 2 + colon),
    };
  }

  private splitTopLevel(text: string, separator: string): string[] {
    const parts: string[] = [];
    let start = 0;
    let depth = 0;
    let inString = false;
    for (let i = 0; i < text.length; i += 1) {
      const char = text[i];
      if (char === '"' && text[i - 1] !== "\\") inString = !inString;
      if (!inString && "({[".includes(char ?? "")) depth += 1;
      if (!inString && ")}]".includes(char ?? "")) depth -= 1;
      if (!inString && depth === 0 && text.startsWith(separator, i)) {
        parts.push(text.slice(start, i).trim());
        start = i + separator.length;
      }
    }
    parts.push(text.slice(start).trim());
    return parts.filter(Boolean);
  }

  private topLevelIndexOf(text: string, needle: string): number {
    let depth = 0;
    let inString = false;
    for (let i = 0; i < text.length; i += 1) {
      const char = text[i];
      if (char === '"' && text[i - 1] !== "\\") inString = !inString;
      if (!inString && "({[".includes(char ?? "")) depth += 1;
      if (!inString && ")}]".includes(char ?? "")) depth -= 1;
      if (!inString && depth === 0 && text.startsWith(needle, i)) return i;
    }
    return -1;
  }

  private topLevelOperatorIndex(
    text: string,
    operators: string[],
    fromRight = false,
  ): number {
    let depth = 0;
    let inString = false;
    const start = fromRight ? text.length - 1 : 0;
    const end = fromRight ? -1 : text.length;
    const step = fromRight ? -1 : 1;
    for (let i = start; i !== end; i += step) {
      const char = text[i];
      if (char === '"' && text[i - 1] !== "\\") inString = !inString;
      if (inString) continue;
      if (fromRight) {
        if (")}]".includes(char ?? "")) depth += 1;
        if ("({[".includes(char ?? "")) depth -= 1;
      } else {
        if ("({[".includes(char ?? "")) depth += 1;
        if (")}]".includes(char ?? "")) depth -= 1;
      }
      if (depth !== 0) continue;
      for (const operator of operators) {
        const operatorIndex = fromRight ? i - operator.length + 1 : i;
        if (operatorIndex < 0 || !text.startsWith(operator, operatorIndex)) continue;
        if ((operator === "+" || operator === "-") && this.isUnarySign(text, operatorIndex)) {
          continue;
        }
        return operatorIndex;
      }
    }
    return -1;
  }

  private isUnarySign(text: string, index: number): boolean {
    let previous = index - 1;
    while (previous >= 0 && /\s/.test(text[previous] ?? "")) previous -= 1;
    if (previous < 0) return true;
    return "([{:,+-*/%<>!=&|?".includes(text[previous] ?? "");
  }

  private skipWhitespaceAndSeparators(text: string, index: number): number {
    let next = index;
    while (next < text.length && /[\s;]/.test(text[next] ?? "")) next += 1;
    return next;
  }

  private unquote(text: string): string {
    return text.slice(1, -1).replace(/\\"/g, '"').replace(/\\n/g, "\n");
  }
}

export function parseCustomSidebarSwift(
  source: string,
  context: JsonTemplateContext,
  previews: WorkspacePreview[],
  stateValues: Record<string, SwiftSidebarStateValue> = {},
): CustomSidebarSwiftDocument {
  const time = new Date().toLocaleTimeString([], { hour12: false });
  const events = context.events ?? emptyCustomSidebarEventsContext();
  return new SwiftSidebarParser(
    source,
    {
      root: { ...context, events, clock: { time }, workspaces: previews },
      __stateValues: stateValues,
    },
  ).parse();
}

export function interpolateCustomSidebarTemplate(
  template: string | undefined,
  context: TemplateContext,
): string {
  if (template === undefined) return "";
  return template.replace(
    /\{([A-Za-z][A-Za-z0-9_]*)(?:\.([A-Za-z][A-Za-z0-9_]*))?\}/g,
    (match, key, field) => {
      const root = context[key as keyof TemplateContext];
      const value =
        field === undefined
          ? root
          : typeof root === "object" && root !== null
            ? (root as unknown as Record<string, unknown>)[field]
            : undefined;
      return value === undefined ? match : String(value);
    },
  );
}

export function resolveCustomSidebarActionParams(
  value: unknown,
  context: TemplateContext,
): unknown {
  if (typeof value === "string") {
    return interpolateCustomSidebarTemplate(value, context);
  }
  if (Array.isArray(value)) {
    return value.map((entry) => resolveCustomSidebarActionParams(entry, context));
  }
  if (typeof value === "object" && value !== null) {
    return Object.fromEntries(
      Object.entries(value).map(([key, entry]) => [
        key,
        resolveCustomSidebarActionParams(entry, context),
      ]),
    );
  }
  return value;
}
