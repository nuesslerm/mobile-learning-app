//! The Engine's multiplexed stream of a container without a TTY: frames of an 8-byte header (stream, three reserved bytes, the length
//! as a big-endian u32) and that many bytes of text. Each frame is decoded alone, so a character cut between two frames is replaced.

const HEADER_LEN: usize = 8;

/// The text of the frame at the start of `buf` and the bytes it takes, or none when `buf` ends inside the frame or its header.
fn next_frame(buf: &[u8]) -> Option<(String, usize)> {
    let header = buf.get(..HEADER_LEN)?;
    let size = usize::try_from(u32::from_be_bytes([header[4], header[5], header[6], header[7]])).ok()?;
    let end = HEADER_LEN.checked_add(size)?;
    let payload = buf.get(HEADER_LEN..end)?;
    Some((String::from_utf8_lossy(payload).into_owned(), end))
}

/// The text of a whole body: its frames joined, whatever their stream, and an incomplete last frame dropped.
pub fn demux(buf: &[u8]) -> String {
    let mut text = String::new();
    let mut offset = 0;
    while let Some((frame, taken)) = next_frame(&buf[offset..]) {
        text.push_str(&frame);
        offset += taken;
    }
    text
}

/// Demuxes a stream that arrives in chunks of any size: a frame can straddle two of them, so an incomplete tail waits for the next.
#[derive(Debug, Default)]
pub struct Demuxer {
    pending: Vec<u8>,
}

impl Demuxer {
    pub fn new() -> Self {
        Self::default()
    }

    /// The texts of the frames `chunk` completes, in order; a frame with no text is left out.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<String> {
        self.pending.extend_from_slice(chunk);
        let mut texts = Vec::new();
        let mut offset = 0;
        while let Some((frame, taken)) = next_frame(&self.pending[offset..]) {
            if !frame.is_empty() {
                texts.push(frame);
            }
            offset += taken;
        }
        self.pending.drain(..offset);
        texts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(stream: u8, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![stream, 0, 0, 0];
        bytes.extend_from_slice(&u32::try_from(payload.len()).expect("a small frame").to_be_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    fn fed_in_pieces(wire: &[u8], piece: usize) -> Vec<String> {
        let mut demuxer = Demuxer::new();
        wire.chunks(piece).flat_map(|chunk| demuxer.feed(chunk)).collect()
    }

    #[test]
    fn given_frames_of_both_streams_when_a_body_is_demuxed_then_their_texts_are_joined_in_order() {
        let body = [frame(1, b"started\n"), frame(2, b"warn: slow\n")].concat();
        assert_eq!(demux(&body), "started\nwarn: slow\n");
    }

    #[test]
    fn given_any_stream_byte_when_a_body_is_demuxed_then_the_text_is_kept() {
        let body = [frame(0, b"in|"), frame(7, b"x|"), frame(1, b"out")].concat();
        assert_eq!(demux(&body), "in|x|out");
    }

    #[test]
    fn given_an_empty_body_when_demuxed_then_the_text_is_empty() {
        assert_eq!(demux(&[]), "");
    }

    #[test]
    fn given_a_body_ending_inside_a_frame_or_a_header_when_demuxed_then_the_incomplete_tail_is_dropped() {
        let cut_frame = [frame(1, b"a"), frame(2, b""), frame(1, b"b"), vec![1, 0, 0, 0, 0, 0, 0, 9, 0x63]].concat();
        assert_eq!(demux(&cut_frame), "ab");
        assert_eq!(demux(&[frame(1, b"hi"), vec![1, 2, 3]].concat()), "hi");
        assert_eq!(demux(&[1, 0, 0, 0, 0, 0, 0]), "");
    }

    #[test]
    fn given_a_frame_that_declares_four_gigabytes_when_demuxed_then_it_never_completes_and_nothing_after_it_is_read() {
        let body = [vec![1, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0x61], frame(1, b"later")].concat();
        assert_eq!(demux(&body), "");
        assert_eq!(fed_in_pieces(&body, 4), Vec::<String>::new());
    }

    #[test]
    fn given_invalid_utf8_in_a_frame_when_demuxed_then_each_maximal_bad_run_becomes_one_replacement_character() {
        let cases: [(&[u8], &str); 13] = [
            (&[0xc0, 0x80], "\u{fffd}\u{fffd}"),
            (&[0x80], "\u{fffd}"),
            (&[0xed, 0xa0, 0x80], "\u{fffd}\u{fffd}\u{fffd}"),
            (&[0xf4, 0x90, 0x80, 0x80], "\u{fffd}\u{fffd}\u{fffd}\u{fffd}"),
            (&[0xf0, 0x9f, 0x98], "\u{fffd}"),
            (&[0xe2, 0x82], "\u{fffd}"),
            (&[0xe2, 0x41], "\u{fffd}A"),
            (&[0xff, 0xfe], "\u{fffd}\u{fffd}"),
            (&[0xef, 0xbb, 0xbf, 0x61], "\u{feff}a"),
            (&[0xf0, 0x9f, 0x98, 0x80], "\u{1f600}"),
            (&[0xf5, 0x80], "\u{fffd}\u{fffd}"),
            (&[0xe0, 0x80, 0x80], "\u{fffd}\u{fffd}\u{fffd}"),
            (&[0x61, 0xe2, 0x82], "a\u{fffd}"),
        ];
        for (bytes, want) in cases {
            assert_eq!(demux(&frame(1, bytes)), want, "{bytes:02x?}");
        }
    }

    #[test]
    fn given_a_character_cut_across_two_frames_when_demuxed_then_each_half_is_decoded_alone_and_replaced() {
        let cut = [frame(1, b"a\xe2\x82"), frame(1, b"\xacb")].concat();
        assert_eq!(demux(&cut), "a\u{fffd}\u{fffd}b");
        let mut demuxer = Demuxer::new();
        let texts: Vec<String> = [&cut[..11], &cut[11..]].into_iter().flat_map(|chunk| demuxer.feed(chunk)).collect();
        assert_eq!(texts, ["a\u{fffd}", "\u{fffd}b"]);
    }

    #[test]
    fn given_chunks_of_any_size_when_fed_then_the_texts_are_those_of_the_frames_with_text() {
        let wire = [frame(1, b"line one\n"), frame(2, "line two \u{20ac}\n".as_bytes()), frame(1, b""), frame(1, b"z")].concat();
        let want = ["line one\n", "line two \u{20ac}\n", "z"];
        for piece in 1..=wire.len() {
            assert_eq!(fed_in_pieces(&wire, piece), want, "chunks of {piece} bytes");
        }
    }

    #[test]
    fn given_a_frame_cut_in_its_header_or_its_text_when_fed_then_it_comes_out_with_the_chunk_that_completes_it() {
        let mut demuxer = Demuxer::new();
        assert_eq!(demuxer.feed(&[frame(1, b"one"), frame(1, b"two")[..9].to_vec()].concat()), ["one"]);
        assert_eq!(demuxer.feed(&frame(1, b"two")[9..]), ["two"]);
        assert!(demuxer.feed(&[1, 0, 0]).is_empty());
        assert!(demuxer.feed(&[0, 0, 0, 0, 2, 0x68]).is_empty());
        assert_eq!(demuxer.feed(&[0x69]), ["hi"]);
    }

    #[test]
    fn given_only_frames_without_text_when_fed_then_nothing_comes_out() {
        assert!(Demuxer::new().feed(&[frame(1, b""), frame(2, b"")].concat()).is_empty());
    }

    #[test]
    fn given_the_engines_frame_cut_into_three_chunks_when_fed_then_the_text_is_whole() {
        let wire = [frame(1, b"line one\n"), frame(2, "line two \u{20ac}\n".as_bytes())].concat();
        let euro = wire.len() - 3;
        let mut demuxer = Demuxer::new();
        let texts: Vec<String> = [&wire[..5], &wire[5..euro], &wire[euro..]].into_iter().flat_map(|chunk| demuxer.feed(chunk)).collect();
        assert_eq!(texts.concat(), "line one\nline two \u{20ac}\n");
    }
}
