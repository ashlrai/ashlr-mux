//! JSONC preprocessing and format-preserving object editing.
//!
//! Faithful 1:1 Rust port of the Swift `Sources/JSONCParser.swift`, which
//! defines two Foundation namespaces:
//!
//! * `enum JSONCParser` — turns a raw JSONC config byte stream into strict JSON
//!   bytes (encoding detection, BOM stripping, comment stripping, trailing-comma
//!   normalization). Ported in [`parser`].
//! * `enum JSONCObjectEditor` — sets a (possibly nested) object property while
//!   preserving comments, indentation, newline style, and trailing-comma state.
//!   Ported in [`editor`].
//!
//! ## Swift source map
//!
//! | Swift symbol (`Sources/JSONCParser.swift`) | Rust item |
//! | --- | --- |
//! | `JSONCParser.preprocess(data:)` (line 4) | [`preprocess`] |
//! | `JSONCParser.source(data:)` (line 12) | [`source`] |
//! | `JSONCParser.detectedJSONEncoding(for:)` (line 57) | [`detected_json_encoding`] |
//! | `JSONCParser.isLineTerminator(_:)` (line 80) | [`is_line_terminator`] |
//! | `JSONCParser.stripComments(from:)` (line 88) | [`strip_comments`] |
//! | `JSONCParser.stripTrailingCommas(from:)` (line 156) | [`strip_trailing_commas`] |
//! | `JSONCParser.JSONCError` (line 215) | [`JsoncError`] |
//! | `JSONCObjectEditor.setNestedObjectProperty(...)` (line 234) | [`set_nested_object_property`] |
//!
//! ## Sanctioned platform divergences
//!
//! * Swift `Data` -> Rust `&[u8]` / `Vec<u8>`.
//! * Swift `String.Index` (extended-grapheme-cluster cursor) -> `usize` index
//!   into a `Vec<char>` (Unicode scalars). The only grapheme cluster combining a
//!   JSON-structural character is `"\r\n"` (one `Character` in Swift, two scalars
//!   in Rust); every algorithm here either appends scalars verbatim (byte-
//!   identical output) or branches on line terminators via [`is_line_terminator`],
//!   whose CR/LF-first-scalar check is indistinguishable between the split and
//!   combined representations. See the [`parser`] and [`editor`] module docs.
//! * Swift `String.Encoding` -> the [`Encoding`] enum. The [`source`] fallback
//!   omits Foundation's `NSString.stringEncoding(for:)` multi-encoding lossy
//!   heuristic (Foundation-only): per spec it defaults to UTF-8 and otherwise
//!   returns [`JsoncError::InvalidTextEncoding`].
//! * Swift `LocalizedError` -> a `thiserror` enum ([`JsoncError`]) whose `Display`
//!   strings are transcribed verbatim from `errorDescription`.
//! * Foundation `JSONEncoder` / `JSONDecoder` (used by the editor to quote and
//!   decode JSON string literals) -> `serde_json`, with the editor replicating
//!   `JSONEncoder`'s default `/` -> `\/` escaping. See the [`editor`] module docs.

pub mod editor;
pub mod parser;

pub use editor::set_nested_object_property;
pub use parser::{
    detected_json_encoding, is_line_terminator, preprocess, source, strip_comments,
    strip_trailing_commas, Encoding, JsoncError,
};
