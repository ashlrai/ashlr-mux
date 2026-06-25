import Foundation

/// Canonicalizer that mirrors `cmux_golden::canonicalize` (Rust) byte-for-byte.
///
/// Contract (see `crates/cmux-golden/src/lib.rs`):
///   * recursively sort object keys (Unicode-scalar / String order),
///   * uppercase any UUID-shaped string (`8-4-4-4-12` hex, dashes at 8/13/18/23),
///   * render pretty with 2-space indent, `": "` and `","` separators, exactly
///     like `serde_json::to_string_pretty`.
///
/// We do NOT use `JSONSerialization`'s `.prettyPrinted` because its spacing
/// (e.g. it pads arrays/objects differently and escapes `/`) does not match
/// serde_json. This hand-rolled printer is the parity-safe path.
enum Canonicalizer {
    /// Render any JSON-coded value (already a `[String: Any]` / `[Any]` / scalar
    /// tree from `JSONSerialization.jsonObject`) to canonical pretty JSON.
    static func render(_ value: Any) -> String {
        var out = String()
        write(value, indent: 0, into: &out)
        return out
    }

    /// Convenience: take an `Encodable`, route it through `JSONEncoder` +
    /// `JSONSerialization` to get a canonicalization-ready tree, then render.
    static func renderEncodable<T: Encodable>(_ value: T) throws -> String {
        let data = try JSONEncoder().encode(value)
        let tree = try JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed])
        return render(tree)
    }

    // MARK: - Printer

    private static func write(_ value: Any, indent: Int, into out: inout String) {
        switch value {
        case let dict as [String: Any]:
            writeObject(dict, indent: indent, into: &out)
        case let array as [Any]:
            writeArray(array, indent: indent, into: &out)
        case let str as String:
            writeString(canonicalUUID(str) ?? str, into: &out)
        case let num as NSNumber:
            writeNumber(num, into: &out)
        case is NSNull:
            out += "null"
        default:
            // Fallback for bridged scalars not caught above.
            out += "null"
        }
    }

    private static func writeObject(_ dict: [String: Any], indent: Int, into out: inout String) {
        if dict.isEmpty { out += "{}"; return }
        out += "{\n"
        let pad = String(repeating: "  ", count: indent + 1)
        let keys = dict.keys.sorted()  // lexicographic, matches Rust String::cmp
        for (i, key) in keys.enumerated() {
            out += pad
            writeString(key, into: &out)
            out += ": "
            write(dict[key]!, indent: indent + 1, into: &out)
            if i < keys.count - 1 { out += "," }
            out += "\n"
        }
        out += String(repeating: "  ", count: indent) + "}"
    }

    private static func writeArray(_ array: [Any], indent: Int, into out: inout String) {
        if array.isEmpty { out += "[]"; return }
        out += "[\n"
        let pad = String(repeating: "  ", count: indent + 1)
        for (i, item) in array.enumerated() {
            out += pad
            write(item, indent: indent + 1, into: &out)
            if i < array.count - 1 { out += "," }
            out += "\n"
        }
        out += String(repeating: "  ", count: indent) + "]"
    }

    private static func writeNumber(_ num: NSNumber, into out: inout String) {
        // Distinguish Bool from numeric NSNumber (Foundation bridges both).
        if CFGetTypeID(num) == CFBooleanGetTypeID() {
            out += num.boolValue ? "true" : "false"
            return
        }
        // Integers print without a decimal point; serde_json does the same.
        let typeChar = String(cString: num.objCType)
        if typeChar == "c" || typeChar == "i" || typeChar == "s" || typeChar == "l"
            || typeChar == "q" || typeChar == "I" || typeChar == "S" || typeChar == "L"
            || typeChar == "Q"
        {
            out += num.stringValue
        } else {
            // serde_json renders f64 without a trailing `.0` only when integral;
            // match its behavior by using the shortest round-trippable form.
            out += shortestDouble(num.doubleValue)
        }
    }

    private static func shortestDouble(_ d: Double) -> String {
        if d == d.rounded() && abs(d) < 1e15 {
            // serde_json emits e.g. `0.5` but integral doubles as `1.0`? No —
            // serde_json emits integral f64 as `1.0`. We preserve that.
            return String(format: "%.1f", d)
        }
        return String(d)
    }

    private static func writeString(_ s: String, into out: inout String) {
        out += "\""
        for scalar in s.unicodeScalars {
            switch scalar {
            case "\"": out += "\\\""
            case "\\": out += "\\\\"
            case "\n": out += "\\n"
            case "\r": out += "\\r"
            case "\t": out += "\\t"
            case let c where c.value < 0x20:
                out += String(format: "\\u%04x", c.value)
            default:
                out.unicodeScalars.append(scalar)
            }
        }
        out += "\""
    }

    // MARK: - UUID folding

    private static func canonicalUUID(_ text: String) -> String? {
        guard text.count == 36 else { return nil }
        let bytes = Array(text.utf8)
        guard bytes.count == 36,
              bytes[8] == UInt8(ascii: "-"), bytes[13] == UInt8(ascii: "-"),
              bytes[18] == UInt8(ascii: "-"), bytes[23] == UInt8(ascii: "-")
        else { return nil }
        guard let uuid = UUID(uuidString: text) else { return nil }
        let upper = uuid.uuidString  // Foundation emits uppercase
        return upper == text ? nil : upper
    }
}
