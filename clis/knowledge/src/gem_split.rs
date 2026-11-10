//! Splits a repomix markdown pack into parts that each fit one Google Doc (a Doc holds about 1,024,000 characters, counted in UTF-16 units).

fn units(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Repomix opens every packed file with a `## File: <path>` heading; the text before the first one is the pack's preamble.
fn sections(pack: &str) -> Vec<&str> {
    let mut starts: Vec<usize> = pack.match_indices("\n## File: ").map(|(i, _)| i + 1).collect();
    starts.insert(0, 0);
    starts.push(pack.len());
    starts.windows(2).map(|w| &pack[w[0]..w[1]]).filter(|s| !s.is_empty()).collect()
}

/// A section longer than `max` is cut at line boundaries (a single line longer than `max` is cut mid-line, at a char boundary).
fn cut(section: &str, max: usize) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut size = 0;
    for line in section.split_inclusive('\n') {
        let mut rest = line;
        while !rest.is_empty() {
            let room = max - size;
            let n = units(rest);
            if n <= room {
                parts.last_mut().expect("one part").push_str(rest);
                size += n;
                break;
            }
            if size > 0 && n <= max {
                parts.push(String::new());
                size = 0;
                continue;
            }
            let mut take = 0;
            let mut used = 0;
            for c in rest.chars() {
                if used + c.len_utf16() > room {
                    break;
                }
                used += c.len_utf16();
                take += c.len_utf8();
            }
            parts.last_mut().expect("one part").push_str(&rest[..take]);
            parts.push(String::new());
            size = 0;
            rest = &rest[take..];
        }
    }
    parts.retain(|p| !p.is_empty());
    parts
}

/// The pack as consecutive parts of at most `max` UTF-16 units each, packed greedily without splitting a file across parts unless the file alone exceeds `max`.
/// Concatenating the parts gives the pack back.
pub fn split_pack(pack: &str, max: usize) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut size = 0;
    for section in sections(pack) {
        let n = units(section);
        if n > max {
            for piece in cut(section, max) {
                size = units(&piece);
                parts.push(piece);
            }
            continue;
        }
        match parts.last_mut() {
            Some(last) if size + n <= max => {
                last.push_str(section);
                size += n;
            }
            _ => {
                parts.push(section.to_string());
                size = n;
            }
        }
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(files: &[(&str, usize)]) -> String {
        let mut s = String::from("preamble\n");
        for (name, n) in files {
            s.push_str(&format!("\n## File: {name}\n{}\n", "x".repeat(*n)));
        }
        s
    }

    #[test]
    fn given_a_pack_when_split_then_the_parts_join_back_to_it_and_each_fits() {
        let p = pack(&[("a", 400), ("b", 400), ("c", 400), ("d", 100)]);
        let parts = split_pack(&p, 1000);
        assert_eq!(parts.concat(), p);
        assert!(parts.len() > 1);
        assert!(parts.iter().all(|x| units(x) <= 1000));
    }

    #[test]
    fn given_files_when_split_then_no_file_is_cut_across_parts() {
        let p = pack(&[("a", 400), ("b", 400), ("c", 400)]);
        for part in split_pack(&p, 1000) {
            for (i, line) in part.lines().enumerate() {
                if line.starts_with("## File: ") {
                    assert!(part.lines().nth(i + 1).is_some_and(|l| l.starts_with('x')));
                }
            }
        }
    }

    #[test]
    fn given_one_file_over_the_limit_when_split_then_it_is_cut_and_still_fits_and_joins() {
        let mut p = String::from("## File: big\n");
        for _ in 0..50 {
            p.push_str(&"é".repeat(30));
            p.push('\n');
        }
        let parts = split_pack(&p, 400);
        assert_eq!(parts.concat(), p);
        assert!(parts.iter().all(|x| units(x) <= 400));
        assert!(parts.len() > 1);
    }

    #[test]
    fn given_a_line_longer_than_the_limit_when_split_then_it_is_cut_mid_line() {
        let p = format!("## File: wide\n{}\n", "z".repeat(1000));
        let parts = split_pack(&p, 300);
        assert_eq!(parts.concat(), p);
        assert!(parts.iter().all(|x| units(x) <= 300));
    }
}
