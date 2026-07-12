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
    let end = index + 1;
    let known = false;
    for (; end < lines.length; end += 1) {
      const line = lines[end].trimEnd();
      if (line === END) {
        known = true;
        end += 1;
        break;
      }
      if (/^(warning|error)(\[|:)/.test(line)) break;
    }
    if (known) index = end;
    else kept.push(lines[index++]);
  }
  return kept.join("");
}
