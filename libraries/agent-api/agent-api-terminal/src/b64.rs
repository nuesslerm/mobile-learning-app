//! Standard base64 with padding, for the bytes of terminal output.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The bytes of `text`, or none when it is not base64 as `encode` writes it.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for (at, quad) in bytes.chunks(4).enumerate() {
        let last = at == bytes.len() / 4 - 1;
        let pad = quad.iter().rev().take_while(|b| **b == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return None;
        }
        let mut n = 0u32;
        for (i, b) in quad.iter().enumerate() {
            let v = if i >= 4 - pad { 0 } else { ALPHABET.iter().position(|a| a == b)? as u32 };
            n |= v << (18 - 6 * i);
        }
        out.extend_from_slice(&n.to_be_bytes()[1..4 - pad]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_every_length_of_bytes_when_encoded_and_decoded_then_they_come_back_and_the_known_vectors_hold() {
        // Given the RFC 4648 vectors
        for (plain, coded) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
            // Then encoding gives the vector and decoding returns the text
            assert_eq!(encode(plain.as_bytes()), coded);
            assert_eq!(decode(coded), Some(plain.as_bytes().to_vec()));
        }
        // And every byte value survives, and text that is no base64 decodes to none
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(decode(&encode(&all)), Some(all));
        for bad in ["abc", "ab=c", "====", "a*bc", "Zg==Zg=="] {
            assert_eq!(decode(bad), None, "{bad}");
        }
    }
}
