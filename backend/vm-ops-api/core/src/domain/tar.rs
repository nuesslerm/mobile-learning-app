//! A minimal single-file ustar archive for `PUT /containers/:id/archive`: the file's bytes never touch a shell. The modification time
//! is an input, not a clock read.

const BLOCK: usize = 512;

/// The file an archive carries; `new` gives the defaults of an unowned file only its owner reads and writes.
#[derive(Debug, Clone, Copy)]
pub struct SingleFile<'a> {
    pub name: &'a str,
    pub content: &'a [u8],
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
}

impl<'a> SingleFile<'a> {
    pub fn new(name: &'a str, content: &'a [u8]) -> Self {
        Self { name, content, mode: 0o600, uid: 0, gid: 0 }
    }
}

/// `text` at `offset`, cut at `width` bytes: a number too wide for its field loses its last digits, as in the tar Node writes.
fn put(header: &mut [u8; BLOCK], offset: usize, width: usize, text: &[u8]) {
    let kept = text.len().min(width);
    header[offset..offset + kept].copy_from_slice(&text[..kept]);
}

/// `value` in octal, zero-padded to `digits`, and a NUL.
fn octal(value: u64, digits: usize) -> String {
    format!("{value:0>digits$o}\0")
}

/// The longest start of `name` that fits 100 bytes without cutting a character.
fn name_field(name: &str) -> &[u8] {
    let mut end = name.len().min(100);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    &name.as_bytes()[..end]
}

/// The header block, the content padded to whole blocks and the two empty blocks that end an archive.
pub fn build_single_file_tar(file: &SingleFile, mtime_secs: u64) -> Vec<u8> {
    let mut header = [0u8; BLOCK];
    put(&mut header, 0, 100, name_field(file.name));
    put(&mut header, 100, 8, octal(file.mode.into(), 7).as_bytes());
    put(&mut header, 108, 8, octal(file.uid.into(), 7).as_bytes());
    put(&mut header, 116, 8, octal(file.gid.into(), 7).as_bytes());
    put(&mut header, 124, 12, octal(file.content.len() as u64, 11).as_bytes());
    put(&mut header, 136, 12, octal(mtime_secs, 11).as_bytes());
    put(&mut header, 148, 8, b"        ");
    put(&mut header, 156, 1, b"0");
    put(&mut header, 257, 6, b"ustar\0");
    put(&mut header, 263, 2, b"00");
    let checksum: u64 = header.iter().map(|&byte| u64::from(byte)).sum();
    put(&mut header, 148, 8, format!("{checksum:0>6o}\0 ").as_bytes());
    let padded = file.content.len().div_ceil(BLOCK) * BLOCK;
    let mut tar = Vec::with_capacity(BLOCK + padded + 2 * BLOCK);
    tar.extend_from_slice(&header);
    tar.extend_from_slice(file.content);
    tar.resize(BLOCK + padded + 2 * BLOCK, 0);
    tar
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    /// 1700000000123 ms, the instant the Node oracle's clock was pinned to, in whole seconds.
    const MTIME: u64 = 1_700_000_000;

    fn sha256(bytes: &[u8]) -> String {
        hex::encode(Sha256::digest(bytes))
    }

    fn field(tar: &[u8], offset: usize, width: usize) -> String {
        tar[offset..offset + width].iter().map(|&b| char::from(b)).collect()
    }

    fn fill(byte: u8, len: usize) -> Vec<u8> {
        vec![byte; len]
    }

    #[test]
    fn given_the_files_the_service_writes_when_built_then_every_byte_is_the_tar_node_writes_for_the_same_second() {
        let (a512, a513) = (fill(0x61, 512), fill(0x62, 513));
        let (x100, y120, e60, z99e) = ("x".repeat(100), "y".repeat(120), "\u{e9}".repeat(60), format!("{}\u{e9}", "z".repeat(99)));
        let cases: Vec<(&str, SingleFile, usize, &str)> = vec![
            ("a.txt", SingleFile::new("a.txt", b"hello"), 2048, "c12f96d0a0993c0966f6a32078960d1c77b26da8e4d9cd87f717301596ba15e6"),
            (
                "credentials",
                SingleFile { uid: 1001, gid: 1001, ..SingleFile::new(".claude/.credentials.json", br#"{"x":1}"#) },
                2048,
                "ff4fe7dbd526625c71066f450449577668e7014f3c38f94550b20b7ec250a5d9",
            ),
            ("empty", SingleFile { mode: 0o644, ..SingleFile::new("empty", b"") }, 1536, "5495f7e71d08c4c05b47fc2fc342cd4ebd4c43c48cc3dc22b21863e84568eddd"),
            (
                "one block",
                SingleFile { mode: 0o755, uid: 1000, gid: 1000, ..SingleFile::new("block", &a512) },
                2048,
                "216b4da0ffd8610c8f5d7fcacf61c576280a573503de271ce905e94e4faa2995",
            ),
            (
                "block plus one",
                SingleFile { uid: 1001, gid: 1001, ..SingleFile::new("block+1", &a513) },
                2560,
                "f268acc67a988747cd14db69dedd1eb8b3dce002a79954f5977c3ce9eb54ae65",
            ),
            ("name of 100", SingleFile::new(&x100, b"abc"), 2048, "c2f7bba82c7e3ba864b6a7ff2957514403d62dbdb971c236f2fd5f63b9c4104d"),
            ("name of 120", SingleFile::new(&y120, b"abc"), 2048, "81e280acd0b2a9c6ed8a8e1aa4f2b6210cb91343da0524b56a4f239505a9ef77"),
            ("name of 60 multibyte", SingleFile::new(&e60, b"abc"), 2048, "96947d96b6f7dbc975aacb20045288337a9edcbede443533a81fc02f9680416b"),
            ("name cut mid-character", SingleFile::new(&z99e, b"abc"), 2048, "83fa1a9dfdcf1770ddac5989d6646914cc1ecab0f2aa65f34101b4f815711484"),
            (
                "wide ids",
                SingleFile { mode: 0o7777, uid: 2_097_151, gid: 2_097_152, ..SingleFile::new("bigids", b"q") },
                2048,
                "8fdae241bf887f641fd3f5b9bbbfa944099e9a7cbf2c7e5dc2bc8bd5ef22825c",
            ),
            (
                "overflowing fields",
                SingleFile { mode: 0o12_345_670, uid: 0o12_345_670, gid: 0o777_777_777, ..SingleFile::new("overflow", b"q") },
                2048,
                "dd3f53b71be02d1c07e2f6b23eb6c9ff4b635c43648ca0f8c1b17bd3fa6c0bf5",
            ),
        ];
        for (label, file, len, sha) in cases {
            let tar = build_single_file_tar(&file, MTIME);
            assert_eq!((tar.len(), sha256(&tar).as_str()), (len, sha), "{label}");
        }
    }

    #[test]
    fn given_a_file_owned_by_the_agent_shell_user_when_built_then_each_header_field_is_where_ustar_puts_it() {
        let file = SingleFile { uid: 1001, gid: 1001, ..SingleFile::new(".claude/.credentials.json", br#"{"x":1}"#) };
        let tar = build_single_file_tar(&file, MTIME);
        assert_eq!(&tar[..25], b".claude/.credentials.json");
        assert!(tar[25..100].iter().all(|&b| b == 0));
        let fields = [
            (100, 8, "0000600\0"),
            (108, 8, "0001751\0"),
            (116, 8, "0001751\0"),
            (124, 12, "00000000007\0"),
            (136, 12, "14524770400\0"),
            (148, 8, "012611\0 "),
            (156, 1, "0"),
            (257, 6, "ustar\0"),
            (263, 2, "00"),
        ];
        for (offset, width, want) in fields {
            assert_eq!(field(&tar, offset, width), want, "bytes {offset}..{}", offset + width);
        }
        assert!(tar[157..257].iter().all(|&b| b == 0) && tar[265..512].iter().all(|&b| b == 0));
    }

    #[test]
    fn given_content_when_built_then_it_follows_the_header_padded_with_zeros_and_two_empty_blocks_end_the_archive() {
        let content = fill(0x62, 513);
        let tar = build_single_file_tar(&SingleFile::new("f", &content), MTIME);
        assert_eq!(&tar[512..1025], &content[..]);
        assert!(tar[1025..1536].iter().all(|&b| b == 0));
        assert_eq!(tar.len(), 512 + 1024 + 1024);
        assert!(tar[tar.len() - 1024..].iter().all(|&b| b == 0));
        assert_eq!(build_single_file_tar(&SingleFile::new("f", b""), MTIME).len(), 512 + 1024);
        assert_eq!(build_single_file_tar(&SingleFile::new("f", &fill(1, 512)), MTIME).len(), 512 + 512 + 1024);
    }

    #[test]
    fn given_any_header_when_built_then_the_checksum_is_the_sum_of_its_bytes_with_the_field_read_as_spaces() {
        let (big, three) = (fill(7, 1000), SingleFile::new("a", b"abc"));
        let files = [SingleFile { mode: 0o700, ..SingleFile::new("script.sh", b"echo hi\n") }, three, SingleFile::new("big", &big)];
        for file in files {
            let tar = build_single_file_tar(&file, MTIME);
            let mut blank = tar[..512].to_vec();
            blank[148..156].copy_from_slice(b"        ");
            let sum: u64 = blank.iter().map(|&b| u64::from(b)).sum();
            assert_eq!(field(&tar, 148, 8), format!("{sum:0>6o}\0 "), "{}", file.name);
        }
    }

    #[test]
    fn given_the_modification_time_when_built_then_it_is_written_in_octal_seconds_and_nothing_else_changes() {
        let file = SingleFile::new("f", b"abc");
        let (earlier, later) = (build_single_file_tar(&file, 0), build_single_file_tar(&file, MTIME));
        assert_eq!(field(&earlier, 136, 12), "00000000000\0");
        assert_eq!(field(&later, 136, 12), "14524770400\0");
        assert_eq!(earlier[..136], later[..136]);
        assert_eq!(earlier[512..], later[512..]);
    }

    #[test]
    fn given_a_name_longer_than_the_field_when_built_then_it_is_cut_at_100_bytes_without_a_terminator() {
        let tar = build_single_file_tar(&SingleFile::new(&"y".repeat(120), b"abc"), MTIME);
        assert_eq!(&tar[..100], "y".repeat(100).as_bytes());
        assert_eq!(&tar[100..104], b"0000");
    }

    #[test]
    fn given_a_name_whose_100th_byte_is_inside_a_character_when_built_then_the_whole_character_is_left_out() {
        let tar = build_single_file_tar(&SingleFile::new(&format!("{}\u{e9}", "z".repeat(99)), b"abc"), MTIME);
        assert_eq!(&tar[..99], "z".repeat(99).as_bytes());
        assert_eq!(tar[99], 0);
        let multibyte = build_single_file_tar(&SingleFile::new(&"\u{e9}".repeat(60), b"abc"), MTIME);
        assert_eq!(&multibyte[..100], "\u{e9}".repeat(50).as_bytes());
        let astral = build_single_file_tar(&SingleFile::new(&format!("{}\u{1f600}", "a".repeat(97)), b"abc"), MTIME);
        assert_eq!((&astral[..97], astral[97]), ("a".repeat(97).as_bytes(), 0));
    }

    #[test]
    fn given_an_id_or_mode_too_wide_for_its_field_when_built_then_the_field_keeps_its_first_eight_bytes() {
        let file = SingleFile { mode: 0o12_345_670, uid: 0o12_345_670, gid: 0o777_777_777, ..SingleFile::new("overflow", b"q") };
        let tar = build_single_file_tar(&file, MTIME);
        assert_eq!((field(&tar, 100, 8), field(&tar, 108, 8), field(&tar, 116, 8)), ("12345670".into(), "12345670".into(), "77777777".into()));
        let wide = SingleFile { mode: 0o7777, uid: 2_097_152, ..SingleFile::new("w", b"q") };
        assert_eq!(field(&build_single_file_tar(&wide, MTIME), 108, 8), "10000000");
    }

    #[test]
    fn given_a_file_made_with_new_when_built_then_it_is_unowned_and_readable_by_its_owner_only() {
        let file = SingleFile::new("f", b"abc");
        assert_eq!((file.mode, file.uid, file.gid), (0o600, 0, 0));
        assert_eq!(field(&build_single_file_tar(&file, MTIME), 100, 24), "0000600\u{0}0000000\u{0}0000000\u{0}");
    }
}
