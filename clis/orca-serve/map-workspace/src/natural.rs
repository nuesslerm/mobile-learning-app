//! The File Explorer's listing order: directories first, then names in the order of `Intl.Collator('en', {numeric: true})`, ties by code unit.
use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub is_directory: bool,
}

enum Piece<'a> {
    Digits(&'a str),
    Text(&'a str),
}

fn pieces(s: &str) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut digits = None;
    for (i, c) in s.char_indices() {
        let d = c.is_ascii_digit();
        match digits {
            Some(was) if was != d => {
                out.push(if was { Piece::Digits(&s[start..i]) } else { Piece::Text(&s[start..i]) });
                start = i;
            }
            _ => {}
        }
        digits = Some(d);
    }
    if let Some(was) = digits {
        out.push(if was { Piece::Digits(&s[start..]) } else { Piece::Text(&s[start..]) });
    }
    out
}

fn numeric(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.trim_start_matches('0'), b.trim_start_matches('0'));
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

pub fn compare_names(a: &str, b: &str) -> Ordering {
    let (pa, pb) = (pieces(a), pieces(b));
    for (x, y) in pa.iter().zip(pb.iter()) {
        let o = match (x, y) {
            (Piece::Digits(x), Piece::Digits(y)) => numeric(x, y),
            (Piece::Text(x), Piece::Text(y)) => x.to_lowercase().cmp(&y.to_lowercase()),
            (Piece::Digits(_), Piece::Text(_)) => Ordering::Less,
            (Piece::Text(_), Piece::Digits(_)) => Ordering::Greater,
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    pa.len().cmp(&pb.len()).then_with(|| a.encode_utf16().cmp(b.encode_utf16()))
}

pub fn compare(a: &Entry, b: &Entry) -> Ordering {
    b.is_directory.cmp(&a.is_directory).then_with(|| compare_names(&a.name, &b.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_names_with_numbers_and_cases_when_sorted_then_folders_come_first_and_numbers_count_by_value() {
        // Given a listing of files and folders whose names mix numbers and cases
        let mut entries: Vec<Entry> = [("file10.txt", false), ("file9.txt", false), ("Zeta", true), ("alpha", true), ("File2.txt", false)]
            .iter()
            .map(|(n, d)| Entry { name: n.to_string(), is_directory: *d })
            .collect();
        // When it is sorted
        entries.sort_by(compare);
        // Then folders lead, names ignore case, and 9 comes before 10
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["alpha", "Zeta", "File2.txt", "file9.txt", "file10.txt"]);
    }
}
