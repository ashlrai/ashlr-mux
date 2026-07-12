const START = "warning: failed to parse serde attribute";
const END = "  = note: ts-rs failed to parse this attribute. It will be ignored.";

export function filterTsRsDiagnostics(stderr) {
  const lines = stderr.split(/(?<=\n)/);
  const kept = [];
  for (let index = 0; index < lines.length; ) {
    if (lines[index].trimEnd() !== START) {
      kept.push(lines[index++]);
      continue;
    }
    let cursor = index + 1;
    let attribute = "";
    let valid = lines[cursor]?.trimEnd() === "  |";
    cursor += 1;
    if (valid && lines[cursor]?.startsWith("  | #[serde(")) {
      attribute = lines[cursor].slice("  | ".length).trim();
      cursor += 1;
      while (cursor < lines.length && lines[cursor].trimEnd() !== "  |") {
        const line = lines[cursor].trimEnd();
        if (/^(warning|error)(\[|:)/.test(line)) break;
        attribute += line.trim();
        cursor += 1;
      }
    }
    const normalizedAttribute = attribute.replace(/=\s*"/g, '= "');
    const knownAttribute = /^#\[serde\((?:rename = "[^"]+", )?(?:default, )?skip_serializing_if = "(?:Option::is_none|Vec::is_empty|is_false)"\)\]$/.test(normalizedAttribute);
    const complete =
      valid &&
      knownAttribute &&
      lines[cursor]?.trimEnd() === "  |" &&
      lines[cursor + 1]?.trimEnd() === END;
    if (complete) {
      index = cursor + 2;
      continue;
    }
    let preserveThrough = index + 1;
    while (preserveThrough < lines.length) {
      if (lines[preserveThrough++].trimEnd() === END) break;
    }
    kept.push(...lines.slice(index, preserveThrough));
    index = preserveThrough;
  }
  return kept.join("");
}
