export abstract class SwiftSyntaxReader {
  protected readViewStatement(
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

  protected readSimpleStatement(
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

  protected readCall(text: string): { name: string; args: string; end: number } | null {
    const match = text.match(/^\s*([A-Za-z_][A-Za-z0-9_]*)\s*\(/);
    if (!match) return null;
    const open = text.indexOf("(", match[0].length - 1);
    const balanced = this.readBalanced(text, open, "(", ")");
    if (balanced === null) return null;
    return { name: match[1] ?? "", args: balanced.content, end: balanced.end };
  }

  protected readModifierCall(text: string): { name: string; args: string; end: number } | null {
    const call = this.readCall(text);
    if (call !== null) return call;
    const match = text.match(/^\s*([A-Za-z_][A-Za-z0-9_]*)\b/);
    if (!match) return null;
    const end = match[0].length;
    const next = this.skipWhitespaceAndSeparators(text, end);
    return text[next] === "{" ? { name: match[1] ?? "", args: "", end } : null;
  }

  protected readViewCall(text: string): { name: string; args: string; end: number } | null {
    const call = this.readCall(text);
    if (call !== null) return call;
    const match = text.match(/^\s*([A-Za-z_][A-Za-z0-9_]*)\b/);
    if (!match) return null;
    const end = match[0].length;
    const next = this.skipWhitespaceAndSeparators(text, end);
    return text[next] === "{" ? { name: match[1] ?? "", args: "", end } : null;
  }

  protected readTrailingClosure(text: string): { body: string; end: number } | null {
    const start = this.skipWhitespaceAndSeparators(text, 0);
    if (text[start] !== "{") return null;
    const balanced = this.readBalanced(text, start, "{", "}");
    return balanced === null ? null : { body: balanced.content, end: balanced.end };
  }

  protected readTrailingClosureSequence(
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

  protected readBalanced(
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

  protected splitClosureParameter(body: string): { params: string[]; body: string } {
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

  protected escapeRegExp(value: string): string {
    return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  }

  protected extractNamedClosure(args: string, label: string): string | null {
    const index = args.indexOf(`${label}:`);
    if (index < 0) return null;
    const open = args.indexOf("{", index);
    const block = open >= 0 ? this.readBalanced(args, open, "{", "}") : null;
    return block?.content ?? null;
  }

  protected splitTernary(text: string): { condition: string; truthy: string; falsy: string } | null {
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

  protected splitTopLevel(text: string, separator: string): string[] {
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

  protected topLevelIndexOf(text: string, needle: string): number {
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

  protected topLevelOperatorIndex(
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

  protected isUnarySign(text: string, index: number): boolean {
    let previous = index - 1;
    while (previous >= 0 && /\s/.test(text[previous] ?? "")) previous -= 1;
    if (previous < 0) return true;
    return "([{:,+-*/%<>!=&|?".includes(text[previous] ?? "");
  }

  protected skipWhitespaceAndSeparators(text: string, index: number): number {
    let next = index;
    while (next < text.length && /[\s;]/.test(text[next] ?? "")) next += 1;
    return next;
  }

  protected unquote(text: string): string {
    return text.slice(1, -1).replace(/\\"/g, '"').replace(/\\n/g, "\n");
  }
}
