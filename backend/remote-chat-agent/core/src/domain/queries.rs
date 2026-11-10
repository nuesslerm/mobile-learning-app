//! What the agent API reads across threads, as T3 reads it (`references/t3code`, `orchestration-v2/ThreadSearch.ts`, `getThreadFamily`,
//! `getOlderSettledThreads` and the archived shell): a search that ranks title matches first and omits archived threads, a thread's family under
//! its root, and the shelves of archived and settled threads in pages. Pure over the read model.

use super::ids::ms_from_iso;
use super::projector::ReadModel;
use super::types::Thread;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitSource {
    Title,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub thread: String,
    pub title: String,
    pub source: HitSource,
    pub snippet: Option<String>,
    pub at_ms: Option<u64>,
}

/// The characters a snippet runs from the match, and the lead before it.
const SNIPPET_LENGTH: usize = 240;
const SNIPPET_LEAD: usize = 24;

fn lower(text: &str) -> Vec<char> {
    text.chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect()
}

fn find(haystack: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|&at| haystack[at..at + needle.len()] == *needle)
}

/// Up to 240 characters from a few words before the match at `start`, whitespace collapsed and emphasis marks dropped, with an ellipsis where the
/// text was cut (`buildSearchSnippet`).
pub fn snippet(text: &str, start: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut from = start.saturating_sub(SNIPPET_LEAD);
    if from > 0 {
        if let Some(space) = chars[from..start.min(chars.len())].iter().position(|c| c.is_whitespace()) {
            from += space + 1;
        }
    }
    let raw_end = chars.len().min(from + SNIPPET_LENGTH * 2);
    let shown: String = chars[from.min(chars.len())..raw_end].iter().collect::<String>().replace("**", "").replace('`', "").split_whitespace().collect::<Vec<_>>().join(" ");
    let shown: Vec<char> = shown.chars().collect();
    let end = shown.len().min(SNIPPET_LENGTH - 2);
    let cut = end < shown.len() || raw_end < chars.len();
    format!("{}{}{}", if from > 0 { "…" } else { "" }, shown[..end].iter().collect::<String>(), if cut { "…" } else { "" })
}

/// How early a thread's title starts the words: the sum over the words of 0 for a title that begins with it, 1 where it starts a word, 2 inside one.
fn title_score(title: &[char], words: &[Vec<char>]) -> Option<usize> {
    let mut score = 0;
    for word in words {
        let at = find(title, word)?;
        score += match at {
            0 => 0,
            _ if !title[at - 1].is_alphanumeric() => 1,
            _ => 2,
        };
    }
    Some(score)
}

fn searchable(thread: &Thread) -> bool {
    thread.archived_at.is_none() && thread.deleted_at.is_none() && thread.lifecycle.lineage.as_ref().is_none_or(|l| l.relationship.as_deref() != Some("subagent"))
}

/// The threads whose title, or a message, holds every word of `query`, at most `limit`: the title matches first, best score first and the most
/// recently active first among equals, then one hit for each other thread with the newest message that matches, newest first. Archived and deleted
/// threads and subagents are left out.
pub fn search(model: &ReadModel, query: &str, limit: usize) -> Vec<SearchHit> {
    let words: Vec<Vec<char>> = query.split_whitespace().map(lower).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut candidates: Vec<&Thread> = model.threads.values().filter(|t| model.neutral.contains_key(&t.id) && searchable(t)).collect();
    candidates.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then_with(|| a.id.cmp(&b.id)));
    let mut titled: Vec<(usize, &Thread)> = candidates.iter().filter_map(|t| title_score(&lower(&t.title), &words).map(|score| (score, *t))).collect();
    titled.sort_by_key(|(score, _)| *score);
    let mut hits: Vec<SearchHit> = titled.iter().map(|(_, t)| SearchHit { thread: t.id.clone(), title: t.title.clone(), source: HitSource::Title, snippet: None, at_ms: None }).collect();
    let mut by_message: Vec<(u64, SearchHit)> = Vec::new();
    for thread in candidates.iter().filter(|t| !titled.iter().any(|(_, titled)| titled.id == t.id)) {
        let Some(neutral) = model.neutral.get(&thread.id) else { continue };
        for turn in neutral.turns.iter().rev() {
            let at_ms = ms_from_iso(&turn.started_at).unwrap_or(0);
            let found = [(HitSource::Assistant, &turn.reply), (HitSource::User, &turn.prompt)].into_iter().find_map(|(source, text)| {
                let folded = lower(text);
                let starts: Option<Vec<usize>> = words.iter().map(|w| find(&folded, w)).collect();
                starts.map(|s| (source, snippet(text, s.into_iter().min().unwrap_or(0))))
            });
            if let Some((source, text)) = found {
                by_message.push((at_ms, SearchHit { thread: thread.id.clone(), title: thread.title.clone(), source, snippet: Some(text), at_ms: Some(at_ms) }));
                break;
            }
        }
    }
    by_message.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.thread.cmp(&b.1.thread)));
    hits.extend(by_message.into_iter().map(|(_, hit)| hit));
    hits.truncate(limit);
    hits
}

fn root_of(thread: &Thread) -> &str {
    thread.lifecycle.lineage.as_ref().map_or(thread.id.as_str(), |l| l.root.as_str())
}

/// The root of `thread`'s family and the ids of the live threads under it (not archived, not deleted), the oldest first. A thread nobody knows has no
/// root and no family; a deleted one still names its root.
pub fn family(model: &ReadModel, thread: &str) -> (Option<String>, Vec<String>) {
    let Some(known) = model.threads.get(thread).filter(|t| model.neutral.contains_key(&t.id)) else { return (None, Vec::new()) };
    let root = root_of(known).to_string();
    let mut members: Vec<&Thread> = model.threads.values().filter(|t| model.neutral.contains_key(&t.id) && root_of(t) == root && t.archived_at.is_none() && t.deleted_at.is_none()).collect();
    members.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));
    (Some(root), members.into_iter().map(|t| t.id.clone()).collect())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shelf {
    Archived,
    Settled,
}

/// Where a page of a shelf starts: below `at_ms`, and after `thread` among the threads at that time (ids ascending); a cursor with no thread
/// leaves out every thread at `at_ms`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    pub at_ms: u64,
    pub thread: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShelfRow {
    pub thread: String,
    pub at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShelfPage {
    pub rows: Vec<ShelfRow>,
    /// The cursor of the page after this one; none when this is the last.
    pub next: Option<Cursor>,
    /// How many threads the shelf holds after the request's cursor.
    pub total: usize,
}

fn archived_at(thread: &Thread) -> Option<u64> {
    thread.archived_at.as_deref().and_then(ms_from_iso)
}

/// A page of at most `limit` threads of the shelf, newest first (the archive or settle time descending, then ids ascending), after `before`. A
/// `limit` of 0 reads only the count. The archived shelf holds archived threads that are not deleted; the settled one the threads a person settled
/// that are neither archived nor deleted.
pub fn shelf(model: &ReadModel, which: Shelf, before: Option<&Cursor>, limit: usize) -> ShelfPage {
    let mut rows: Vec<ShelfRow> = model
        .threads
        .values()
        .filter(|t| model.neutral.contains_key(&t.id) && t.deleted_at.is_none())
        .filter_map(|t| match which {
            Shelf::Archived => archived_at(t).map(|at_ms| ShelfRow { thread: t.id.clone(), at_ms }),
            Shelf::Settled if t.archived_at.is_none() => t.lifecycle.settle.as_ref().filter(|s| s.state.as_deref() == Some("settled")).and_then(|s| s.settled_at_ms).map(|at_ms| ShelfRow { thread: t.id.clone(), at_ms }),
            Shelf::Settled => None,
        })
        .collect();
    rows.sort_by(|a, b| b.at_ms.cmp(&a.at_ms).then_with(|| a.thread.cmp(&b.thread)));
    if let Some(cursor) = before {
        rows.retain(|r| r.at_ms < cursor.at_ms || (r.at_ms == cursor.at_ms && cursor.thread.as_ref().is_some_and(|after| r.thread > *after)));
    }
    let total = rows.len();
    rows.truncate(limit);
    let next = rows.last().filter(|_| rows.len() < total).map(|last| Cursor { at_ms: last.at_ms, thread: Some(last.thread.clone()) });
    ShelfPage { rows, next, total }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::lifecycle::tests::Rig;
    use crate::domain::lifecycle::SettleTo;
    use crate::domain::types::Body;

    fn ids(hits: &[SearchHit]) -> Vec<(&str, HitSource)> {
        hits.iter().map(|h| (h.thread.as_str(), h.source)).collect()
    }

    #[test]
    fn given_threads_with_matching_titles_and_messages_when_searched_then_title_matches_come_first_by_how_early_the_word_starts_and_archived_ones_are_left_out() {
        // Given a thread that starts with the word, one with it inside a word, one with it after a space, a thread whose reply holds it, and an archived thread titled with it
        let mut rig = Rig::new();
        for (id, title) in [("a", "login page"), ("b", "relogin flow"), ("c", "fix login bug"), ("d", "something else"), ("e", "login archived")] {
            rig.thread(id, Some("w1"), Some("claudeAgent"));
            rig.event(id, "thread.meta-updated", serde_json::json!({"title": title, "updatedAt": "2023-11-14T22:13:20.000Z"}));
        }
        rig.turn("d", "u1", "how", "You can login with a token here", "completed");
        rig.run("e", Body::LifecycleArchive { archive: true }).unwrap();
        // When searched for "login"
        let hits = search(&rig.model, "login", 10);
        // Then the titles come first (begins, word start, inside), then the message hit with its snippet, and the archived thread is not among them
        assert_eq!(ids(&hits), [("a", HitSource::Title), ("c", HitSource::Title), ("b", HitSource::Title), ("d", HitSource::Assistant)]);
        assert_eq!(hits[3].snippet.as_deref(), Some("You can login with a token here"));
        // And a search needs every word, is case-blind, and a limit cuts the list after the best
        assert_eq!(ids(&search(&rig.model, "LOGIN bug", 10)), [("c", HitSource::Title)]);
        assert_eq!(search(&rig.model, "login", 2).len(), 2);
        assert!(search(&rig.model, "   ", 5).is_empty() && search(&rig.model, "zebra", 5).is_empty());
    }

    #[test]
    fn given_a_long_message_when_a_match_is_cut_into_a_snippet_then_it_starts_a_few_words_before_the_match_and_ends_in_ellipses_at_the_cut() {
        // Given a text with the match far in, and text with emphasis marks
        let text = format!("{} needle {}", "word ".repeat(30), "tail ".repeat(100));
        let at = text.find("needle").unwrap();
        // When cut, Then it opens with an ellipsis, holds the match near its start, ends in one, and is no longer than the cap and its marks
        let cut = snippet(&text, at);
        assert!(cut.starts_with('…') && cut.ends_with('…'), "{cut}");
        assert!(cut.contains("needle") && cut.chars().count() <= SNIPPET_LENGTH, "{cut}");
        assert!(cut.find("needle").unwrap() < 40, "{cut}");
        assert_eq!(snippet("**bold** and `code`", 0), "bold and code");
    }

    #[test]
    fn given_a_thread_and_its_forks_when_the_family_is_read_then_it_is_the_live_threads_under_one_root_and_a_deleted_or_unknown_thread_is_told_apart() {
        // Given a root with two forks, a fork of a fork, an archived fork, a deleted one, and a thread of another family
        let mut rig = Rig::new();
        rig.thread("root", Some("w1"), Some("claudeAgent"));
        rig.turn("root", "u1", "x", "y", "completed");
        for (id, from) in [("f1", "root"), ("f2", "root"), ("g1", "f1"), ("old", "root"), ("gone", "root")] {
            rig.now_ms += 1_000;
            rig.run(id, Body::Fork { source: from.into(), at_turn: None, title: None }).unwrap();
        }
        rig.thread("other", Some("w1"), Some("claudeAgent"));
        rig.run("old", Body::LifecycleArchive { archive: true }).unwrap();
        rig.run("gone", Body::LifecycleDelete { only_if_unused: false }).unwrap();
        // When the family is read from a fork of a fork, from the deleted one, and for a thread nobody has
        let from_leaf = family(&rig.model, "g1");
        let from_deleted = family(&rig.model, "gone");
        // Then both name the root and list the live members (oldest first), the unknown thread has none, and the other family is its own
        assert_eq!(from_leaf, (Some("root".into()), vec!["root".to_string(), "f1".into(), "f2".into(), "g1".into()]));
        assert_eq!(from_deleted, from_leaf);
        assert_eq!(family(&rig.model, "nobody"), (None, vec![]));
        assert_eq!(family(&rig.model, "other"), (Some("other".into()), vec!["other".to_string()]));
    }

    #[test]
    fn given_fifty_six_settled_threads_when_the_shelf_is_paged_then_every_thread_is_on_exactly_one_page_newest_first_and_the_cursor_continues_where_a_page_ended() {
        // Given 56 threads settled at times that repeat (so the cursor must break ties by id), one archived and one never settled
        let mut rig = Rig::new();
        let base = rig.now_ms;
        for n in 0..56u64 {
            let id = format!("t{n:02}");
            rig.thread(&id, Some("w1"), Some("claudeAgent"));
            rig.run(&id, Body::SettleSet { to: SettleTo::Settled, until_ms: None, at_ms: Some(base + (n / 2) * 1_000) }).unwrap();
        }
        rig.thread("never", Some("w1"), Some("claudeAgent"));
        rig.run("t00", Body::LifecycleArchive { archive: true }).unwrap();
        // When the settled shelf is read in pages of 20 until it ends, and a page of 0
        let mut seen: Vec<String> = Vec::new();
        let mut cursor: Option<Cursor> = None;
        let mut sizes = Vec::new();
        loop {
            let page = shelf(&rig.model, Shelf::Settled, cursor.as_ref(), 20);
            sizes.push((page.rows.len(), page.total));
            seen.extend(page.rows.iter().map(|r| r.thread.clone()));
            match page.next {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        let count = shelf(&rig.model, Shelf::Settled, None, 0);
        // Then 55 threads (the archived one is not settled any more on the shelf) came in pages of 20, 20 and 15, once each, newest settle first
        assert_eq!(sizes, [(20, 55), (20, 35), (15, 15)]);
        let mut unique = seen.clone();
        unique.sort();
        unique.dedup();
        assert_eq!((seen.len(), unique.len()), (55, 55));
        assert_eq!(seen[0], "t54");
        assert_eq!(seen[1], "t55");
        assert!(!seen.contains(&"t00".to_string()) && !seen.contains(&"never".to_string()));
        // And a page of 0 only counts, and a cursor with no thread leaves out the threads at its time
        assert_eq!((count.rows.len(), count.next, count.total), (0, None, 55));
        let strict = shelf(&rig.model, Shelf::Settled, Some(&Cursor { at_ms: base + 27_000, thread: None }), 100);
        assert_eq!(strict.rows.iter().map(|r| r.at_ms).max(), Some(base + 26_000));
    }

    #[test]
    fn given_archived_threads_when_the_archived_shelf_is_read_then_it_lists_them_newest_archive_first_and_not_the_deleted() {
        // Given three threads archived at different times, one of them deleted afterwards, and one never archived
        let mut rig = Rig::new();
        for id in ["a", "b", "c", "d"] {
            rig.thread(id, Some("w1"), Some("claudeAgent"));
        }
        for id in ["a", "b", "c"] {
            rig.now_ms += 1_000;
            rig.run(id, Body::LifecycleArchive { archive: true }).unwrap();
        }
        rig.run("b", Body::LifecycleDelete { only_if_unused: false }).unwrap();
        // When the archived shelf is read, Then it holds c then a
        let page = shelf(&rig.model, Shelf::Archived, None, 10);
        assert_eq!((page.rows.iter().map(|r| r.thread.as_str()).collect::<Vec<_>>(), page.total, page.next), (vec!["c", "a"], 2, None));
    }
}
