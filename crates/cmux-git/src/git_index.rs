//! Pure git `index` (staging area) parsing.
//!
//! Headless port of the filesystem-free core of
//! `Packages/macOS/CmuxGit/Sources/CmuxGit/Parsing/GitMetadataService+Index.swift`.
//! Parses a git `index` byte buffer (versions 2, 3, and 4) into a
//! [`GitIndexSnapshot`], handling v3 extended flags, v4 path prefix-compression,
//! assume-unchanged / skip-worktree exclusion, 8-byte entry padding, the
//! trailing 20-byte checksum, an FNV-1a content signature over path+mode+object
//! ID, and repository-relative path validation.
//!
//! The I/O shell is intentionally **excluded** (host wiring, not ported here):
//! `gitTrackedChangesSnapshot` (lstat dirty compare), `gitlinkWorktreeCommit`
//! (submodule fs resolve), and the `Data(contentsOf:)` file-read wrapper. The
//! Swift entry point `gitIndexSnapshot(indexURL:)` reads the file then calls the
//! pure parser; here [`git_index_snapshot`] takes the already-read bytes.
//!
//! Model ports: `Model/GitIndexEntryStat.swift`, `Model/GitIndexSnapshot.swift`.

/// One parsed entry from a git `index` file: the path plus the cached stat
/// fields git uses to decide whether the working-tree file changed.
///
/// Port of `Model/GitIndexEntryStat.swift:9-28`. Fields mirror the on-disk
/// index entry layout (big-endian), narrowed to what dirty-detection needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitIndexEntryStat {
    /// Repository-relative path of the tracked entry.
    pub path: String,
    /// The git mode word (e.g. `0o100644`, `0o100755`, `0o120000`, or the
    /// `0o160000` gitlink mode for a submodule).
    pub mode: u32,
    /// The 40-hex-char object ID (blob SHA, or the recorded submodule commit).
    pub object_id: String,
    /// Cached `mtime` seconds, truncated to 32 bits as git stores it.
    pub mtime_seconds: u32,
    /// Cached `mtime` nanoseconds, truncated to 32 bits as git stores it.
    pub mtime_nanoseconds: u32,
    /// Cached file size, truncated to 32 bits as git stores it.
    pub size: u32,
}

/// The result of parsing a git `index` file: the tracked entries plus two
/// signatures used to detect change.
///
/// Port of `Model/GitIndexSnapshot.swift:7-18`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitIndexSnapshot {
    /// The tracked entries that participate in dirty-detection (assume-unchanged
    /// and skip-worktree entries are excluded).
    pub entries: Vec<GitIndexEntryStat>,
    /// The raw index trailing-checksum signature (changes on any index rewrite).
    pub signature: String,
    /// A content signature over all entries' paths, modes, and object IDs that
    /// is stable across index rewrites which don't change tracked content.
    pub content_signature: String,
}

const HEX_ALPHABET: &[u8; 16] = b"0123456789abcdef";

/// Parses a git `index` byte buffer (versions 2, 3, and 4) into a snapshot.
///
/// Port of `gitIndexSnapshot(indexURL:)`
/// (`GitMetadataService+Index.swift:63-166`), minus the `Data(contentsOf:)`
/// file read. Returns `None` for an absent, truncated, or unsupported-version
/// index.
pub fn git_index_snapshot(bytes: &[u8]) -> Option<GitIndexSnapshot> {
    // `data.count >= 32` (Swift:64).
    if bytes.len() < 32 {
        return None;
    }
    // Magic `DIRC` (Swift:68).
    if bytes[0] != 0x44 || bytes[1] != 0x49 || bytes[2] != 0x52 || bytes[3] != 0x43 {
        return None;
    }
    let version = read_big_endian_u32(bytes, 4);
    if version != 2 && version != 3 && version != 4 {
        return None;
    }
    let entry_count = read_big_endian_u32(bytes, 8) as usize;
    let content_end = bytes.len() - 20;
    let mut offset: usize = 12;
    let mut entries: Vec<GitIndexEntryStat> = Vec::new();
    let mut content_entries: Vec<GitIndexEntryStat> = Vec::new();
    let mut previous_path_bytes: Vec<u8> = Vec::new();

    for _ in 0..entry_count {
        // `guard offset + 62 <= contentEnd` (Swift:85).
        if offset + 62 > content_end {
            return None;
        }
        let entry_start = offset;
        let mtime_seconds = read_big_endian_u32(bytes, offset + 8);
        let mtime_nanoseconds = read_big_endian_u32(bytes, offset + 12);
        let mode = read_big_endian_u32(bytes, offset + 24);
        let size = read_big_endian_u32(bytes, offset + 36);
        let object_id = hex_string(&bytes[(offset + 40)..(offset + 60)]);
        let flags = read_big_endian_u16(bytes, offset + 60);
        let path_length = (flags & 0x0fff) as usize;
        let has_extended_flags = version >= 3 && (flags & 0x4000) != 0;
        let mut extended_flags: u16 = 0;
        offset += 62;
        if has_extended_flags {
            // `guard offset + 2 <= contentEnd` (Swift:98).
            if offset + 2 > content_end {
                return None;
            }
            extended_flags = read_big_endian_u16(bytes, offset);
            offset += 2;
        }

        let path_bytes: Vec<u8> = if version == 4 {
            // v4 prefix-compression (Swift:104-114).
            let strip_length = read_git_index_v4_path_strip_length(bytes, &mut offset)?;
            if strip_length > previous_path_bytes.len() {
                return None;
            }
            let suffix_start = offset;
            while offset < content_end && bytes[offset] != 0 {
                offset += 1;
            }
            if offset >= content_end {
                return None;
            }
            let mut assembled: Vec<u8> =
                previous_path_bytes[..previous_path_bytes.len() - strip_length].to_vec();
            assembled.extend_from_slice(&bytes[suffix_start..offset]);
            assembled
        } else {
            // Non-v4: fixed pathLength unless the 0x0fff sentinel (Swift:116-126).
            let path_start = offset;
            if path_length < 0x0fff {
                offset += path_length;
                if offset >= content_end {
                    return None;
                }
            } else {
                while offset < content_end && bytes[offset] != 0 {
                    offset += 1;
                }
                if offset >= content_end {
                    return None;
                }
            }
            bytes[path_start..offset].to_vec()
        };

        // `String(data:encoding:.utf8)`, non-empty, valid path (Swift:129-133).
        let path = match std::str::from_utf8(&path_bytes) {
            Ok(s) if !s.is_empty() && is_valid_index_entry_path(s) => s.to_string(),
            _ => return None,
        };
        previous_path_bytes = path_bytes;
        let entry_stat = GitIndexEntryStat {
            path,
            mode,
            object_id,
            mtime_seconds,
            mtime_nanoseconds,
            size,
        };
        content_entries.push(entry_stat.clone());

        // Exclude assume-unchanged / skip-worktree from dirty-tracking entries
        // (Swift:145-150).
        let assume_unchanged_flag: u16 = 0x8000;
        let skip_worktree_extended_flag: u16 = 0x4000;
        if (flags & assume_unchanged_flag) == 0
            && (extended_flags & skip_worktree_extended_flag) == 0
        {
            entries.push(entry_stat);
        }

        offset += 1; // null terminator (Swift:152).
        if version != 4 {
            // 8-byte entry padding (Swift:153-157).
            let entry_length = offset - entry_start;
            let padding = (8 - (entry_length % 8)) % 8;
            offset += padding;
        }
    }

    let checksum = hex_string(&bytes[(bytes.len() - 20)..]);
    Some(GitIndexSnapshot {
        entries,
        signature: checksum,
        content_signature: git_index_content_signature(&content_entries),
    })
}

/// An FNV-1a content signature over each entry's path, mode, and object ID
/// (stat-independent), used to detect tracked-content changes across index
/// rewrites.
///
/// Port of `gitIndexContentSignature(entries:)`
/// (`GitMetadataService+Index.swift:171-202`).
pub fn git_index_content_signature(entries: &[GitIndexEntryStat]) -> String {
    let mut hash: u64 = 14_695_981_039_346_656_037;

    let append_byte = |hash: &mut u64, byte: u8| {
        *hash ^= byte as u64;
        *hash = hash.wrapping_mul(1_099_511_628_211);
    };

    let append_u32 = |hash: &mut u64, value: u32| {
        append_byte(hash, ((value >> 24) & 0xff) as u8);
        append_byte(hash, ((value >> 16) & 0xff) as u8);
        append_byte(hash, ((value >> 8) & 0xff) as u8);
        append_byte(hash, (value & 0xff) as u8);
    };

    let append_str = |hash: &mut u64, value: &str| {
        for byte in value.as_bytes() {
            append_byte(hash, *byte);
        }
    };

    // `UInt32(truncatingIfNeeded: entries.count)` (Swift:192).
    append_u32(&mut hash, entries.len() as u32);
    for entry in entries {
        append_str(&mut hash, &entry.path);
        append_byte(&mut hash, 0);
        append_u32(&mut hash, entry.mode);
        append_byte(&mut hash, 0);
        append_str(&mut hash, &entry.object_id);
        append_byte(&mut hash, 0);
    }
    fixed_width_hex_string(hash)
}

/// Lowercase hex encoding of a byte slice.
///
/// Port of `gitIndexHexString(_:)` (`GitMetadataService+Index.swift:204-212`).
fn hex_string(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        encoded.push(HEX_ALPHABET[(byte >> 4) as usize] as char);
        encoded.push(HEX_ALPHABET[(byte & 0x0f) as usize] as char);
    }
    encoded
}

/// A `u64` as a zero-padded 16-char lowercase hex string.
///
/// Port of `gitIndexFixedWidthHexString(_:)`
/// (`GitMetadataService+Index.swift:214-222`).
fn fixed_width_hex_string(value: u64) -> String {
    let mut encoded = [b'0'; 16];
    let mut remaining = value;
    for index in (0..16).rev() {
        encoded[index] = HEX_ALPHABET[(remaining & 0x0f) as usize];
        remaining >>= 4;
    }
    // All bytes are ASCII hex digits.
    String::from_utf8(encoded.to_vec()).expect("hex bytes are ASCII")
}

/// Maps a stat mode word to the git index mode word for comparison
/// (regular/executable file or symlink), or `None` for other file types.
///
/// Port of `gitIndexComparableMode(for:)`
/// (`GitMetadataService+Index.swift:242-252`). Uses the numeric POSIX mode
/// constants directly (`S_IFMT`=`0o170000`, `S_IFREG`=`0o100000`,
/// `S_IFLNK`=`0o120000`) so it is cross-platform; the executable bits mask is
/// `0o111`.
pub fn git_index_comparable_mode(stat_mode: u32) -> Option<u32> {
    let file_type = stat_mode & 0o170000;
    match file_type {
        0o100000 => Some(if (stat_mode & 0o111) == 0 {
            0o100644
        } else {
            0o100755
        }),
        0o120000 => Some(0o120000),
        _ => None,
    }
}

/// Whether an index entry path is one git would accept: repository-relative
/// (not absolute) and free of `..` traversal components.
///
/// Port of `isValidIndexEntryPath(_:)`
/// (`GitMetadataService+Index.swift:262-265`). Swift's `split(separator:"/")`
/// omits empty subsequences, so empty components between slashes are ignored.
pub fn is_valid_index_entry_path(path: &str) -> bool {
    if path.starts_with('/') {
        return false;
    }
    !path.split('/').filter(|s| !s.is_empty()).any(|s| s == "..")
}

/// Decodes a git index v4 path strip-length varint, advancing `offset`.
///
/// Port of `readGitIndexV4PathStripLength(_:offset:)`
/// (`GitMetadataService+Index.swift:278-296`). Git's index v4 path compression
/// uses `varint.c`'s encode/decode pair, whose continuation bytes increment the
/// accumulated value before shifting (`value += 1` at Swift:290).
pub fn read_git_index_v4_path_strip_length(bytes: &[u8], offset: &mut usize) -> Option<usize> {
    if *offset >= bytes.len() {
        return None;
    }
    let mut byte = bytes[*offset];
    *offset += 1;
    let mut value = (byte & 0x7f) as usize;
    while (byte & 0x80) != 0 {
        if *offset >= bytes.len() {
            return None;
        }
        value += 1;
        byte = bytes[*offset];
        *offset += 1;
        value = (value << 7) + (byte & 0x7f) as usize;
    }
    Some(value)
}

/// Reads a big-endian `u16` at `offset`.
///
/// Port of `readBigEndianUInt16(_:at:)`
/// (`GitMetadataService+Index.swift:299-301`).
pub fn read_big_endian_u16(bytes: &[u8], offset: usize) -> u16 {
    ((bytes[offset] as u16) << 8) | (bytes[offset + 1] as u16)
}

/// Reads a big-endian `u32` at `offset`.
///
/// Port of `readBigEndianUInt32(_:at:)`
/// (`GitMetadataService+Index.swift:304-309`).
pub fn read_big_endian_u32(bytes: &[u8], offset: usize) -> u32 {
    ((bytes[offset] as u32) << 24)
        | ((bytes[offset + 1] as u32) << 16)
        | ((bytes[offset + 2] as u32) << 8)
        | (bytes[offset + 3] as u32)
}

// ---------------------------------------------------------------------------
// Tests — ported from GitMetadataServiceTests.swift:152-231 & 258-277, plus the
// GitIndexFixture DIRC byte serializer (Fixtures/GitIndexFixture.swift) ported
// as a Rust test helper. Parity-risk inputs (v4 multi-byte varint, the
// non-v4 pathLength<0x0fff branch, excluded assume-unchanged/skip-worktree
// entries) are pinned directly.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Test-only builder for a binary git `index` file. Port of
    /// `Fixtures/GitIndexFixture.swift`. Supports versions 2, 3, and 4.
    struct GitIndexFixture {
        version: u32,
        entries: Vec<FixtureEntry>,
        trailer: Vec<u8>,
    }

    #[derive(Clone)]
    struct FixtureEntry {
        path: String,
        mode: u32,
        object_id: String,
        mtime_seconds: u32,
        mtime_nanoseconds: u32,
        size: u32,
        assume_unchanged: bool,
        skip_worktree: bool,
    }

    impl FixtureEntry {
        /// Mirrors `GitIndexFixture.Entry`'s Swift defaults (Fixture:11-20).
        fn new(path: &str) -> Self {
            FixtureEntry {
                path: path.to_string(),
                mode: 0o100644,
                object_id: "a".repeat(40),
                mtime_seconds: 1,
                mtime_nanoseconds: 0,
                size: 0,
                assume_unchanged: false,
                skip_worktree: false,
            }
        }
    }

    impl GitIndexFixture {
        fn new(version: u32, entries: Vec<FixtureEntry>) -> Self {
            GitIndexFixture {
                version,
                entries,
                trailer: vec![0xAB; 20],
            }
        }

        fn with_trailer(version: u32, entries: Vec<FixtureEntry>, trailer: Vec<u8>) -> Self {
            GitIndexFixture {
                version,
                entries,
                trailer,
            }
        }

        /// Port of `GitIndexFixture.data()` (Fixture:32-88).
        fn data(&self) -> Vec<u8> {
            let mut bytes: Vec<u8> = Vec::new();
            bytes.extend_from_slice(b"DIRC");
            bytes.extend_from_slice(&self.version.to_be_bytes());
            bytes.extend_from_slice(&(self.entries.len() as u32).to_be_bytes());

            let mut previous_path: Vec<u8> = Vec::new();
            for entry in &self.entries {
                let entry_start = bytes.len();
                // ctime sec/nsec (unused).
                bytes.extend_from_slice(&0u32.to_be_bytes());
                bytes.extend_from_slice(&0u32.to_be_bytes());
                // mtime sec/nsec (+8, +12).
                bytes.extend_from_slice(&entry.mtime_seconds.to_be_bytes());
                bytes.extend_from_slice(&entry.mtime_nanoseconds.to_be_bytes());
                // dev, ino (+16, +20; unused).
                bytes.extend_from_slice(&0u32.to_be_bytes());
                bytes.extend_from_slice(&0u32.to_be_bytes());
                // mode (+24).
                bytes.extend_from_slice(&entry.mode.to_be_bytes());
                // uid, gid (+28, +32; unused).
                bytes.extend_from_slice(&0u32.to_be_bytes());
                bytes.extend_from_slice(&0u32.to_be_bytes());
                // size (+36).
                bytes.extend_from_slice(&entry.size.to_be_bytes());
                // object id (+40, 20 bytes).
                bytes.extend_from_slice(&hex_bytes(&entry.object_id));
                // flags (+60, 2 bytes).
                let path_bytes = entry.path.as_bytes().to_vec();
                let mut flags = std::cmp::min(path_bytes.len(), 0x0fff) as u16;
                if entry.assume_unchanged {
                    flags |= 0x8000;
                }
                let uses_extended_flags = self.version >= 3 && entry.skip_worktree;
                if uses_extended_flags {
                    flags |= 0x4000;
                }
                bytes.extend_from_slice(&flags.to_be_bytes());
                if uses_extended_flags {
                    let extended: u16 = if entry.skip_worktree { 0x4000 } else { 0 };
                    bytes.extend_from_slice(&extended.to_be_bytes());
                }

                if self.version == 4 {
                    let strip_length = common_prefix_length(&previous_path, &path_bytes);
                    bytes.extend_from_slice(&v4_strip_length_varint(
                        previous_path.len() - strip_length,
                    ));
                    bytes.extend_from_slice(&path_bytes[strip_length..]);
                    bytes.push(0);
                } else {
                    bytes.extend_from_slice(&path_bytes);
                    bytes.push(0);
                    let entry_length = bytes.len() - entry_start;
                    let padding = (8 - (entry_length % 8)) % 8;
                    bytes.extend(std::iter::repeat_n(0u8, padding));
                }
                previous_path = path_bytes;
            }

            bytes.extend_from_slice(&self.trailer);
            bytes
        }
    }

    /// Port of `GitIndexFixture.commonPrefixLength` (Fixture:90-96).
    fn common_prefix_length(lhs: &[u8], rhs: &[u8]) -> usize {
        let mut count = 0;
        while count < lhs.len() && count < rhs.len() && lhs[count] == rhs[count] {
            count += 1;
        }
        count
    }

    /// Port of `GitIndexFixture.hexBytes` (Fixture:106-118): 2 hex chars → 1
    /// byte, up to 20 bytes, zero-padded to 20.
    fn hex_bytes(hex: &str) -> Vec<u8> {
        let chars: Vec<char> = hex.chars().collect();
        let mut result: Vec<u8> = Vec::new();
        let mut index = 0;
        while index < chars.len() && result.len() < 20 {
            let next = std::cmp::min(index + 2, chars.len());
            let chunk: String = chars[index..next].iter().collect();
            if let Ok(byte) = u8::from_str_radix(&chunk, 16) {
                result.push(byte);
            }
            index = next;
        }
        while result.len() < 20 {
            result.push(0);
        }
        result
    }

    /// Port of `GitIndexFixture.v4StripLengthVarint` (Fixture:122-133): the
    /// inverse of [`read_git_index_v4_path_strip_length`].
    fn v4_strip_length_varint(value: usize) -> Vec<u8> {
        let mut bytes: Vec<u8> = Vec::new();
        let mut remaining = value;
        bytes.push((remaining & 0x7f) as u8);
        remaining >>= 7;
        while remaining != 0 {
            remaining -= 1;
            bytes.insert(0, 0x80 | (remaining & 0x7f) as u8);
            remaining >>= 7;
        }
        bytes
    }

    /// `indexVersionFourDecodesPrefixCompressedPaths` (Tests:152-165).
    #[test]
    fn index_version_four_decodes_prefix_compressed_paths() {
        let entries = vec![
            FixtureEntry::new("src/alpha.swift"),
            FixtureEntry::new("src/alphabet.swift"), // shares "src/alpha" prefix
            FixtureEntry::new("src/beta.swift"),
        ];
        let data = GitIndexFixture::new(4, entries).data();
        let snapshot = git_index_snapshot(&data).expect("snapshot");
        let paths: Vec<&str> = snapshot.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            ["src/alpha.swift", "src/alphabet.swift", "src/beta.swift"]
        );
    }

    /// `indexVersionFourMultiByteStripLength` (Tests:167-181). A 200-char shared
    /// prefix forces a multi-byte strip-length varint.
    #[test]
    fn index_version_four_multi_byte_strip_length() {
        let long_prefix = "deep/".repeat(40); // 200 chars
        let entries = vec![
            FixtureEntry::new(&format!("{long_prefix}first.txt")),
            FixtureEntry::new(&format!("{long_prefix}second.txt")),
        ];
        let data = GitIndexFixture::new(4, entries).data();
        let snapshot = git_index_snapshot(&data).expect("snapshot");
        let paths: Vec<String> = snapshot.entries.iter().map(|e| e.path.clone()).collect();
        assert_eq!(
            paths,
            [
                format!("{long_prefix}first.txt"),
                format!("{long_prefix}second.txt"),
            ]
        );
    }

    /// `indexHexFieldsDecodeWithoutChangingObjectAndSignatureText`
    /// (Tests:183-200).
    #[test]
    fn index_hex_fields_decode_without_changing_object_and_signature_text() {
        let object_id = "000102030405060708090a0b0c0d0e0f10111213";
        let trailer: Vec<u8> = (0x14u8..=0x27u8).collect();
        let entries = vec![FixtureEntry {
            object_id: object_id.to_string(),
            ..FixtureEntry::new("tracked.txt")
        }];
        let data = GitIndexFixture::with_trailer(2, entries, trailer).data();
        let snapshot = git_index_snapshot(&data).expect("snapshot");
        let object_ids: Vec<&str> = snapshot
            .entries
            .iter()
            .map(|e| e.object_id.as_str())
            .collect();
        assert_eq!(object_ids, [object_id]);
        assert_eq!(
            snapshot.signature,
            "1415161718191a1b1c1d1e1f2021222324252627"
        );
    }

    /// `contentSignatureIgnoresStatOnlyChanges` (Tests:204-216): the signature
    /// covers path + mode + objectID only.
    #[test]
    fn content_signature_ignores_stat_only_changes() {
        let base = GitIndexEntryStat {
            path: "a.txt".to_string(),
            mode: 0o100644,
            object_id: "b".repeat(40),
            mtime_seconds: 10,
            mtime_nanoseconds: 0,
            size: 5,
        };
        let restated = GitIndexEntryStat {
            path: "a.txt".to_string(),
            mode: 0o100644,
            object_id: "b".repeat(40),
            mtime_seconds: 999,
            mtime_nanoseconds: 7,
            size: 9999,
        };
        assert_eq!(
            git_index_content_signature(&[base]),
            git_index_content_signature(&[restated])
        );
    }

    /// `contentSignatureChangesWithObjectID` (Tests:218-231).
    #[test]
    fn content_signature_changes_with_object_id() {
        let base = GitIndexEntryStat {
            path: "a.txt".to_string(),
            mode: 0o100644,
            object_id: "b".repeat(40),
            mtime_seconds: 1,
            mtime_nanoseconds: 0,
            size: 0,
        };
        let changed = GitIndexEntryStat {
            path: "a.txt".to_string(),
            mode: 0o100644,
            object_id: "c".repeat(40),
            mtime_seconds: 1,
            mtime_nanoseconds: 0,
            size: 0,
        };
        assert_ne!(
            git_index_content_signature(&[base]),
            git_index_content_signature(&[changed])
        );
    }

    /// `indexWithTraversalPathIsRejected` (Tests:258-267).
    #[test]
    fn index_with_traversal_path_is_rejected() {
        let entries = vec![
            FixtureEntry::new("ok.txt"),
            FixtureEntry::new("../escape.txt"),
        ];
        let data = GitIndexFixture::new(2, entries).data();
        assert_eq!(git_index_snapshot(&data), None);
    }

    /// `indexWithAbsolutePathIsRejected` (Tests:269-277).
    #[test]
    fn index_with_absolute_path_is_rejected() {
        let entries = vec![FixtureEntry::new("/etc/passwd")];
        let data = GitIndexFixture::new(2, entries).data();
        assert_eq!(git_index_snapshot(&data), None);
    }

    // ---- Parity-risk & primitive coverage pinned directly ----

    /// The non-v4 `pathLength < 0x0fff` branch: a short path uses the flags-
    /// encoded length verbatim (Swift:117-119). Exercised via a v2 fixture,
    /// whose flags carry the true path length.
    #[test]
    fn version_two_short_path_uses_flag_length() {
        let data = GitIndexFixture::new(2, vec![FixtureEntry::new("file.txt")]).data();
        let snapshot = git_index_snapshot(&data).expect("snapshot");
        assert_eq!(snapshot.entries.len(), 1);
        assert_eq!(snapshot.entries[0].path, "file.txt");
    }

    /// assume-unchanged and skip-worktree entries are excluded from `entries`
    /// but still contribute to the content signature via `contentEntries`
    /// (Swift:143-150). A v3 fixture carries the skip-worktree extended flag.
    #[test]
    fn assume_unchanged_and_skip_worktree_are_excluded_from_tracked_entries() {
        let entries = vec![
            FixtureEntry::new("kept.txt"),
            FixtureEntry {
                assume_unchanged: true,
                ..FixtureEntry::new("assumed.txt")
            },
            FixtureEntry {
                skip_worktree: true,
                ..FixtureEntry::new("skipped.txt")
            },
        ];
        let data = GitIndexFixture::new(3, entries).data();
        let snapshot = git_index_snapshot(&data).expect("snapshot");
        let paths: Vec<&str> = snapshot.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["kept.txt"]);

        // Content signature covers all three (contentEntries), so it differs
        // from a snapshot of only the kept entry.
        let kept_only = GitIndexFixture::new(3, vec![FixtureEntry::new("kept.txt")]).data();
        let kept_snapshot = git_index_snapshot(&kept_only).expect("snapshot");
        assert_ne!(snapshot.content_signature, kept_snapshot.content_signature);
    }

    /// Rejects non-`DIRC` magic, unsupported versions, and undersized buffers.
    #[test]
    fn rejects_malformed_headers() {
        assert_eq!(git_index_snapshot(&[]), None);
        assert_eq!(git_index_snapshot(&[0u8; 40]), None); // wrong magic
        let mut bad_version = GitIndexFixture::new(2, vec![FixtureEntry::new("a.txt")]).data();
        bad_version[7] = 5; // version 5, unsupported
        assert_eq!(git_index_snapshot(&bad_version), None);
    }

    /// Empty index (zero entries) parses and yields the trailer signature.
    #[test]
    fn empty_index_parses() {
        let trailer: Vec<u8> = (0x14u8..=0x27u8).collect();
        let data = GitIndexFixture::with_trailer(2, vec![], trailer).data();
        let snapshot = git_index_snapshot(&data).expect("snapshot");
        assert!(snapshot.entries.is_empty());
        assert_eq!(
            snapshot.signature,
            "1415161718191a1b1c1d1e1f2021222324252627"
        );
    }

    /// [`is_valid_index_entry_path`] parity: absolute and `..`-traversal paths
    /// are rejected; empty components between slashes are ignored (Swift:262-265).
    #[test]
    fn valid_index_entry_path_rules() {
        assert!(is_valid_index_entry_path("src/main.rs"));
        assert!(!is_valid_index_entry_path("/etc/passwd"));
        assert!(!is_valid_index_entry_path("../escape"));
        assert!(!is_valid_index_entry_path("a/../b"));
        assert!(is_valid_index_entry_path("a//b")); // empty component omitted
        assert!(is_valid_index_entry_path("a..b")); // not a `..` component
    }

    /// The v4 varint continuation quirk (`value += 1` before shift, Swift:290)
    /// round-trips with the fixture encoder for a range of strip lengths.
    #[test]
    fn v4_varint_round_trips() {
        for value in [0usize, 1, 127, 128, 129, 200, 300, 16_383, 16_384] {
            let encoded = v4_strip_length_varint(value);
            let mut offset = 0;
            let decoded =
                read_git_index_v4_path_strip_length(&encoded, &mut offset).expect("decode");
            assert_eq!(decoded, value, "value {value}");
            assert_eq!(offset, encoded.len(), "consumed all bytes for {value}");
        }
    }

    /// [`git_index_comparable_mode`] parity (Swift:242-252).
    #[test]
    fn comparable_mode_mapping() {
        assert_eq!(git_index_comparable_mode(0o100644), Some(0o100644));
        assert_eq!(git_index_comparable_mode(0o100755), Some(0o100755));
        assert_eq!(git_index_comparable_mode(0o100600), Some(0o100644));
        assert_eq!(git_index_comparable_mode(0o120777), Some(0o120000)); // symlink
        assert_eq!(git_index_comparable_mode(0o040000), None); // directory
        assert_eq!(git_index_comparable_mode(0o160000), None); // gitlink
    }
}
