import type {
  SwiftAngleValue,
  SwiftDateIntervalValue,
  SwiftDateValue,
  SwiftGridItemValue,
  SwiftMeasurementValue,
  SwiftPointValue,
  SwiftRectValue,
  SwiftSizeValue,
  SwiftUnitPointValue,
  WorkspacePreview,
} from "./customSidebarModel";
import type { SwiftParseScope } from "./customSidebarSwiftParser";
import { SwiftSyntaxReader } from "./SwiftSyntaxReader";

export abstract class SwiftExpressionEvaluator extends SwiftSyntaxReader {
  protected abstract evalCondition(condition: string, scope: SwiftParseScope): boolean;
  protected abstract evalExpression(expression: string, scope: SwiftParseScope): string;
  protected abstract firstPositionalExpression(args: string): string | undefined;
  protected abstract invokeValueFunction(
    name: string,
    args: string,
    scope: SwiftParseScope,
  ): unknown;
  protected abstract namedArg(args: string | undefined, label: string): string | undefined;
  protected abstract namedStringArg(
    args: string,
    label: string,
    scope: SwiftParseScope,
  ): string | undefined;
  protected abstract swiftAlignmentToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined;
  protected abstract toNumber(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): number | undefined;

  protected evalValue(expression: string, scope: SwiftParseScope): unknown {
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

  protected unwrapParenthesizedExpression(expression: string): string {
    let trimmed = expression.trim();
    while (trimmed.startsWith("(")) {
      const balanced = this.readBalanced(trimmed, 0, "(", ")");
      if (balanced === null || balanced.end !== trimmed.length) break;
      trimmed = balanced.content.trim();
    }
    return trimmed;
  }

  protected evalLogicalExpression(expression: string, scope: SwiftParseScope): boolean | undefined {
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

  protected evalComparisonExpression(
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

  protected evalArithmeticExpression(expression: string, scope: SwiftParseScope): unknown {
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

  protected truthy(value: unknown): boolean {
    if (typeof value === "boolean") return value;
    if (typeof value === "number") return value !== 0;
    if (typeof value === "string") return value !== "" && value !== "false";
    return value !== undefined && value !== null;
  }

  protected evalComparisonOperand(expression: string, scope: SwiftParseScope): unknown {
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

  protected resolvePath(path: string, scope: SwiftParseScope): unknown {
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

  protected resolvePathPart(part: string, scope: SwiftParseScope): unknown {
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

  protected resolveMember(value: unknown, part: string, scope: SwiftParseScope): unknown {
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

  protected swiftKeyPathParts(expression: string | undefined): string[] | undefined {
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

  protected resolveKeyPathValue(
    value: unknown,
    parts: string[],
    scope: SwiftParseScope,
  ): unknown {
    return parts.reduce(
      (current, part) => this.resolveMember(current, part, scope),
      value,
    );
  }

  protected compareProjectedValues(left: unknown, right: unknown): number {
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

  protected arrayProjection(
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

  protected arrayComparator(
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

  protected evalCollectionLiteral(content: string, scope: SwiftParseScope): unknown {
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

  protected resolveSubscript(
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

  protected resolveArrayMethod(
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

  protected reduceInitialValue(args: string, scope: SwiftParseScope): unknown {
    const initialArg =
      this.splitTopLevel(args, ",").find((arg) => !arg.trim().startsWith("{")) ?? "";
    const trimmed = initialArg.trim();
    if (trimmed.startsWith("into:")) {
      return this.evalValue(trimmed.slice("into:".length), scope);
    }
    return this.evalValue(trimmed, scope);
  }

  protected arrayMethodClosure(
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

  protected scopeWithClosureValues(
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

  protected resolveStringMethod(
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

  protected formatSwiftValue(value: unknown, format: string, scope: SwiftParseScope): string {
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

  protected formatSwiftList(value: unknown[], format: string): string {
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

  protected formatSwiftNumber(value: number, format: string, scope: SwiftParseScope): string {
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

  protected formatSwiftByteCount(value: number, format: string): string {
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

  protected evalMeasurementInitializer(
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

  protected isSwiftMeasurement(value: unknown): value is SwiftMeasurementValue {
    return (
      typeof value === "object" &&
      value !== null &&
      (value as SwiftMeasurementValue).__swiftMeasurement === true &&
      typeof (value as SwiftMeasurementValue).value === "number" &&
      typeof (value as SwiftMeasurementValue).unit === "string"
    );
  }

  protected isSwiftAngle(value: unknown): value is SwiftAngleValue {
    return (
      typeof value === "object" &&
      value !== null &&
      (value as SwiftAngleValue).__swiftAngle === true &&
      typeof (value as SwiftAngleValue).degrees === "number"
    );
  }

  protected isSwiftUnitPoint(value: unknown): value is SwiftUnitPointValue {
    return (
      typeof value === "object" &&
      value !== null &&
      (value as SwiftUnitPointValue).__swiftUnitPoint === true &&
      typeof (value as SwiftUnitPointValue).x === "number" &&
      typeof (value as SwiftUnitPointValue).y === "number"
    );
  }

  protected isSwiftPoint(value: unknown): value is SwiftPointValue {
    return (
      typeof value === "object" &&
      value !== null &&
      (value as SwiftPointValue).__swiftPoint === true &&
      typeof (value as SwiftPointValue).x === "number" &&
      typeof (value as SwiftPointValue).y === "number"
    );
  }

  protected isSwiftSize(value: unknown): value is SwiftSizeValue {
    return (
      typeof value === "object" &&
      value !== null &&
      (value as SwiftSizeValue).__swiftSize === true &&
      typeof (value as SwiftSizeValue).width === "number" &&
      typeof (value as SwiftSizeValue).height === "number"
    );
  }

  protected isSwiftRect(value: unknown): value is SwiftRectValue {
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

  protected swiftGeometryDisplayValue(value: unknown): string | undefined {
    if (this.isSwiftAngle(value)) return String(value.degrees);
    if (this.isSwiftUnitPoint(value)) return value.token ?? `${value.x},${value.y}`;
    if (this.isSwiftPoint(value)) return `${value.x},${value.y}`;
    if (this.isSwiftSize(value)) return `${value.width}x${value.height}`;
    if (this.isSwiftRect(value)) return `${value.x},${value.y},${value.width}x${value.height}`;
    return undefined;
  }

  protected swiftMeasurementUnitToken(expression: string): string {
    return expression
      .trim()
      .replace(/^\./, "")
      .replace(/^(?:UnitLength|UnitDuration|UnitInformationStorage|UnitMass|UnitTemperature)\./, "");
  }

  protected formatSwiftMeasurement(measurement: SwiftMeasurementValue, format: string): string {
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

  protected swiftMeasurementUnitLabel(unit: string, wide: boolean, value: number): string {
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

  protected evalDateInitializer(
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

  protected isSwiftDate(value: unknown): value is SwiftDateValue {
    return (
      typeof value === "object" &&
      value !== null &&
      (value as SwiftDateValue).__swiftDate === true &&
      typeof (value as SwiftDateValue).epochMs === "number"
    );
  }

  protected evalDateInterval(
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

  protected formatSwiftTextDateStyle(value: SwiftDateValue, style: string): string {
    if (style === "time") return this.formatSwiftDate(value, ".time");
    if (style === "relative" || style === "offset") {
      return this.formatSwiftRelativeDuration(value.epochMs - Date.now());
    }
    if (style === "timer") {
      return this.formatSwiftDuration(value.epochMs - Date.now());
    }
    return this.formatSwiftDate(value, ".date");
  }

  protected formatSwiftDate(value: SwiftDateValue, format: string): string {
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

  protected formatSwiftDatePart(date: Date): string {
    return new Intl.DateTimeFormat("en-US", {
      timeZone: "UTC",
      year: "numeric",
      month: "short",
      day: "numeric",
    }).format(date);
  }

  protected formatSwiftTimePart(date: Date): string {
    return new Intl.DateTimeFormat("en-US", {
      timeZone: "UTC",
      hour: "numeric",
      minute: "2-digit",
    }).format(date);
  }

  protected formatSwiftTimerInterval(
    interval: SwiftDateIntervalValue,
    countsDown: boolean,
  ): string {
    const delta = countsDown
      ? interval.end.epochMs - interval.start.epochMs
      : interval.start.epochMs - interval.end.epochMs;
    return this.formatSwiftDuration(delta);
  }

  protected formatSwiftDuration(deltaMs: number): string {
    const totalSeconds = Math.max(0, Math.round(Math.abs(deltaMs) / 1000));
    const hours = Math.floor(totalSeconds / 3600);
    const minutes = Math.floor((totalSeconds % 3600) / 60);
    const seconds = totalSeconds % 60;
    if (hours > 0) {
      return `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
    }
    return `${minutes}:${String(seconds).padStart(2, "0")}`;
  }

  protected formatSwiftRelativeDuration(deltaMs: number): string {
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

  protected evalBuiltinFunction(
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

  protected evalDottedSwiftGeometryLiteral(
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

  protected evalAngleInitializer(
    args: string,
    scope: SwiftParseScope,
  ): SwiftAngleValue | undefined {
    return (
      this.swiftAngleFromDegrees(this.toNumber(this.namedArg(args, "degrees"), scope)) ??
      this.swiftAngleFromRadians(this.toNumber(this.namedArg(args, "radians"), scope)) ??
      this.swiftAngleFromDegrees(this.toNumber(this.firstPositionalExpression(args), scope))
    );
  }

  protected swiftAngleFromDegrees(value: number | undefined): SwiftAngleValue | undefined {
    return value === undefined ? undefined : { __swiftAngle: true, degrees: value };
  }

  protected swiftAngleFromRadians(value: number | undefined): SwiftAngleValue | undefined {
    return value === undefined
      ? undefined
      : { __swiftAngle: true, degrees: value * (180 / Math.PI) };
  }

  protected evalUnitPointInitializer(
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

  protected swiftUnitPointFromToken(token: string): SwiftUnitPointValue | undefined {
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

  protected swiftUnitPointTokenFromCoordinates(x: number, y: number): string | undefined {
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

  protected evalPointInitializer(
    args: string,
    scope: SwiftParseScope,
  ): SwiftPointValue | undefined {
    const x = this.toNumber(this.namedArg(args, "x"), scope) ?? 0;
    const y = this.toNumber(this.namedArg(args, "y"), scope) ?? 0;
    return { __swiftPoint: true, x, y };
  }

  protected evalSizeInitializer(
    args: string,
    scope: SwiftParseScope,
  ): SwiftSizeValue | undefined {
    const width = this.toNumber(this.namedArg(args, "width"), scope) ?? 0;
    const height = this.toNumber(this.namedArg(args, "height"), scope) ?? 0;
    return { __swiftSize: true, width, height };
  }

  protected evalRectInitializer(
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

  protected evalGridItemInitializer(
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

  protected swiftGridItemSize(
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

  protected evalStringFormatInitializer(
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

  protected evalDictionaryInitializer(
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

  protected formatSwiftString(format: string, values: unknown[]): string {
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

  protected formatSwiftSpecifier(
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

  protected interpolateSwiftString(value: string, scope: SwiftParseScope): string {
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

  protected evalSequence(sequence: string, scope: SwiftParseScope): unknown[] | null {
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

  protected scopeWithLoopValue(
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

  protected scopeWithLoopValues(
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

  protected scopeWithForEachValues(
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

  protected isWorkspacePreview(value: unknown): value is WorkspacePreview {
    return (
      typeof value === "object" &&
      value !== null &&
      typeof (value as WorkspacePreview).id === "string" &&
      typeof (value as WorkspacePreview).title === "string"
    );
  }

  protected firstPositionalArg(args: string, scope: SwiftParseScope): string | undefined {
    const first = this.splitTopLevel(args, ",").find(
      (arg) => this.topLevelIndexOf(arg, ":") < 0,
    );
    return first === undefined ? undefined : this.evalExpression(first, scope);
  }

}
