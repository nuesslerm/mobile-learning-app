//! The fixed skeleton of the CI comment. Every section the finished comment has is there from the first render: its heading, then the real content or
//! one placeholder line, so an update replaces a line in place and nothing below it jumps. The headings are defined once, here, for the renderers and the
//! placeholders alike.

pub(crate) const WHERE_TIME: &str = "### ⏱️ Where the time went";
pub(crate) const JOBS: &str = "### Jobs";
pub(crate) const FIXED_COST: &str = "### Fixed cost and the runner pool";
pub(crate) const CHART: &str = "### Minutes per job";
pub(crate) const TREND: &str = "### Trend";
pub(crate) const IDLE: &str = "### Where the idle time is";
pub(crate) const TODO: &str = "### What to do";
pub(crate) const RUNNERS: &str = "### 🖥️ Runners during the run";
pub(crate) const VM: &str = "### 🖥️ VM during the run";
pub(crate) const CACHE: &str = "### 🗄️ Cache";
pub(crate) const CACHE_HEALTH: &str = "### 🗄️ Cache health";
pub(crate) const CACHE_PROBLEMS: &str = "### Cache problems";
pub(crate) const SLOWEST_STEPS: &str = "### Slowest steps per job";

/// The line for data that is not there yet: fixed text per section, so a render never changes size by saying more or less.
pub(crate) fn loading(waits_for: &str) -> String {
    format!("_loading: waits for {waits_for}_")
}

/// The line for a finished run that has nothing to put in the section.
pub(crate) fn none(why: &str) -> String {
    format!("_none: {why}_")
}

/// The placeholder line of a section: `loading` while the run goes on, `none` once it is over.
pub(crate) fn pending(running: bool, waits_for: &str, why_none: &str) -> String {
    if running { loading(waits_for) } else { none(why_none) }
}

/// The section as rendered when it has content, else its heading over the placeholder.
pub(crate) fn slot(heading: &str, section: String, running: bool, waits_for: &str, why_none: &str) -> String {
    if section.is_empty() {
        format!("\n{heading}\n\n{}\n", pending(running, waits_for, why_none))
    } else {
        section
    }
}

/// What the Last run half says when the database holds nothing of the previous run for the section (or there is no previous run).
pub(crate) const NONE_RECORDED: &str = "_none recorded_";

/// The most characters of one Last run half; a longer one is cut at a line and says so.
const LAST_CAP: usize = 2_000;
const CUT: &str = "_shortened: the previous run's first rows are shown_";
const SHORTENED: &str = "_shortened: the comment is near GitHub's size limit_";

/// A section's text without its leading blank lines and its heading line.
pub(crate) fn body_of(section: &str) -> String {
    let text = section.trim_start_matches('\n');
    match text.split_once('\n') {
        Some((first, rest)) if first.starts_with("### ") => rest.trim().to_string(),
        _ if text.starts_with("### ") => String::new(),
        _ => text.trim().to_string(),
    }
}

/// The Last run halves of a comment, held back while it is assembled: the comment is first filled with each half cut to `LAST_CAP`, and when that is over the size
/// limit every half shrinks to one line, so the doubled sections never cost the This run halves their place.
#[derive(Default)]
pub(crate) struct Lasts {
    held: Vec<String>,
}

const OPEN: char = '\u{1}';
const CLOSE: char = '\u{2}';

impl Lasts {
    fn hold(&mut self, content: String) -> String {
        self.held.push(content);
        format!("{OPEN}{}{CLOSE}", self.held.len() - 1)
    }

    /// What the held halves add to the comment at their normal size.
    pub(crate) fn weight(&self) -> usize {
        self.held.iter().map(|h| shortened(h, false).len()).sum()
    }

    /// `out` with every held half in place: cut to `LAST_CAP` lines, or only the one `shortened` line when `minimal`.
    pub(crate) fn fill(&self, out: &str, minimal: bool) -> String {
        let mut result = String::with_capacity(out.len());
        let mut rest = out;
        while let Some(at) = rest.find(OPEN) {
            result.push_str(&rest[..at]);
            let after = &rest[at + OPEN.len_utf8()..];
            let end = after.find(CLOSE).unwrap_or(after.len());
            let item = after[..end].parse::<usize>().ok().and_then(|i| self.held.get(i));
            result.push_str(&item.map_or(String::new(), |h| shortened(h, minimal)));
            rest = after.get(end + CLOSE.len_utf8()..).unwrap_or("");
        }
        result.push_str(rest);
        result
    }
}

fn shortened(content: &str, minimal: bool) -> String {
    if minimal {
        return SHORTENED.to_string();
    }
    if content.len() <= LAST_CAP {
        return content.to_string();
    }
    let mut kept = String::new();
    for line in content.lines() {
        if kept.len() + line.len() + 1 > LAST_CAP {
            break;
        }
        kept.push_str(line);
        kept.push('\n');
    }
    let kept = kept.trim_end();
    let unclosed = kept.matches("<details").count().saturating_sub(kept.matches("</details>").count());
    format!("{kept}{}\n\n{CUT}", "\n</details>".repeat(unclosed))
}

fn last_label(label: Option<&str>) -> String {
    label.map_or("Last run".to_string(), |l| format!("Last run · {l}"))
}

/// The line of `body` that starts with `prefix` and the rest of it; a body of one line that has no such line is all summary (a placeholder).
fn split_summary(body: &str, prefix: &str) -> (String, String) {
    let lines: Vec<&str> = body.lines().collect();
    match lines.iter().position(|l| l.starts_with(prefix)) {
        Some(at) => {
            let rest: Vec<&str> = lines.iter().enumerate().filter(|(i, _)| *i != at).map(|(_, l)| *l).collect();
            (lines[at].to_string(), rest.join("\n").trim().to_string())
        }
        None if lines.len() == 1 => (body.to_string(), String::new()),
        None => (String::new(), body.to_string()),
    }
}

/// What a This run half says while it has no content: what it waits for while the run goes on, why it is empty once it is over.
#[derive(Clone, Copy)]
pub(crate) struct Wait<'a> {
    pub running: bool,
    pub waits_for: &'a str,
    pub why_none: &'a str,
}

impl Wait<'_> {
    fn line(self) -> String {
        pending(self.running, self.waits_for, self.why_none)
    }
}

/// A section as two labelled blocks, the previous run's first (`last` is its label and its section text, or `None` without a previous run), then this run's, which
/// is `waits_for` while the run goes on. The heading and both labels are there from the first render.
pub(crate) fn blocks(held: &mut Lasts, heading: &str, last: Option<(&str, &str)>, this: &str, wait: Wait) -> String {
    let this = Some(body_of(this)).filter(|b| !b.is_empty()).unwrap_or_else(|| wait.line());
    let (label, body) = last.map_or((last_label(None), String::new()), |(label, section)| (last_label(Some(label)), body_of(section)));
    let body = if body.is_empty() { NONE_RECORDED.to_string() } else { held.hold(body) };
    format!("\n{heading}\n\n**{label}**\n\n{body}\n\n**This run**\n\n{this}\n")
}

/// A section as a two-column table `Last run | This run` for its one summary line (the line of the text that starts with `prefix`), and the rest of it, when either
/// run has any, as the two labelled blocks under the table.
pub(crate) fn summary_table(held: &mut Lasts, heading: &str, last: Option<(&str, &str)>, this: &str, prefix: &str, wait: Wait) -> String {
    let this = Some(body_of(this)).filter(|b| !b.is_empty()).unwrap_or_else(|| wait.line());
    let (label, body) = last.map_or((last_label(None), String::new()), |(label, section)| (last_label(Some(label)), body_of(section)));
    let ((last_line, last_rest), (this_line, this_rest)) = (split_summary(&body, prefix), split_summary(&this, prefix));
    let cell = |line: &str, empty: &str| if line.is_empty() { empty.to_string() } else { line.replace('\n', " ").replace('|', "\\|") };
    let mut out = format!("\n{heading}\n\n| {label} | This run |\n|---|---|\n| {} | {} |\n", cell(&last_line, NONE_RECORDED), cell(&this_line, "-"));
    if !last_rest.is_empty() || !this_rest.is_empty() {
        let last_rest = if last_rest.is_empty() { "_none_".to_string() } else { held.hold(last_rest) };
        let waiting = this_line.starts_with("_loading") || this_line.starts_with("_none:");
        let this_rest = match (this_rest.is_empty(), waiting) {
            (false, _) => this_rest,
            (true, true) => this_line.clone(),
            (true, false) => "_none_".to_string(),
        };
        out.push_str(&format!("\n**Last run**\n\n{last_rest}\n\n**This run**\n\n{this_rest}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_an_empty_section_when_slotted_then_a_running_run_says_loading_and_a_finished_one_says_none() {
        // Given a section with no content
        // When it is slotted into a running and a finished comment
        let running = slot(CACHE, String::new(), true, "the first finished job", "no log held a cache event");
        let finished = slot(CACHE, String::new(), false, "the first finished job", "no log held a cache event");
        // Then the heading stays and only a finished run's text has no loading in it
        assert_eq!(running, "\n### 🗄️ Cache\n\n_loading: waits for the first finished job_\n");
        assert_eq!(finished, "\n### 🗄️ Cache\n\n_none: no log held a cache event_\n");
        assert_eq!(slot(CACHE, "\n### 🗄️ Cache\n\nreal\n".to_string(), true, "x", "y"), "\n### 🗄️ Cache\n\nreal\n");
    }

    #[test]
    fn given_two_halves_when_laid_out_then_the_heading_both_labels_and_the_placeholders_are_there_whatever_the_data() {
        // Given a section with no previous run and one still loading, and one with a previous run's text
        let mut held = Lasts::default();
        // When both are laid out as blocks
        let bare = blocks(&mut held, CACHE, None, "", Wait { running: true, waits_for: "the first finished job", why_none: "no log held a cache event" });
        let full = blocks(&mut held, CACHE, Some(("`abc1234`, 2h ago", "\n### 🗄️ Cache\n\nold text\n")), "\n### 🗄️ Cache\n\nnew text\n", Wait { running: false, waits_for: "x", why_none: "y" });
        // Then each has the heading, the Last run label and the This run label in order, and the placeholders are the fixed lines
        assert_eq!(bare, "\n### 🗄️ Cache\n\n**Last run**\n\n_none recorded_\n\n**This run**\n\n_loading: waits for the first finished job_\n");
        assert_eq!(held.fill(&full, false), "\n### 🗄️ Cache\n\n**Last run · `abc1234`, 2h ago**\n\nold text\n\n**This run**\n\nnew text\n");
    }

    #[test]
    fn given_a_long_last_run_half_when_filled_then_it_is_cut_at_a_line_and_says_so_and_a_minimal_fill_keeps_one_line() {
        // Given a held half of 300 table rows
        let mut held = Lasts::default();
        let rows: String = (0..300).map(|i| format!("| row {i} | value |\n")).collect();
        let out = blocks(&mut held, JOBS, Some(("x", &format!("### Jobs\n\n{rows}"))), "", Wait { running: true, waits_for: "w", why_none: "n" });
        // When it is filled normally and minimally
        let normal = held.fill(&out, false);
        let minimal = held.fill(&out, true);
        // Then the normal fill is under the cap with whole rows and the shortened line, and the minimal one has only the line
        assert!(normal.len() < LAST_CAP + 300 && normal.contains("| row 0 | value |") && normal.contains(CUT) && !normal.contains("| row 299 |"), "{normal}");
        assert!(minimal.len() < 200 && minimal.contains(SHORTENED) && !minimal.contains("| row 0 |"), "{minimal}");
        assert!(held.weight() <= LAST_CAP + 100);
    }

    #[test]
    fn given_a_summary_line_when_tabled_then_the_two_runs_share_a_row_and_the_rest_goes_under_labelled_blocks() {
        // Given the previous run's overhead line and steps table, and a run that has only its placeholder
        let mut held = Lasts::default();
        let last = "\n### Where the idle time is\n\n| Step |\n|---|\n| a |\n\n**Overhead**: 20%.\n";
        let out = summary_table(&mut held, IDLE, Some(("`abc1234`, 2h ago", last)), "", "**Overhead**", Wait { running: true, waits_for: "the first step with CPU samples", why_none: "no step has CPU samples" });
        // When it is filled
        let text = held.fill(&out, false);
        // Then the row has the line and the placeholder and the steps table sits under the Last run label
        assert!(text.contains("| Last run · `abc1234`, 2h ago | This run |\n|---|---|\n| **Overhead**: 20%. | _loading: waits for the first step with CPU samples_ |"), "{text}");
        assert!(text.contains("**Last run**\n\n| Step |") && text.contains("**This run**\n\n_loading: waits for the first step with CPU samples_"), "{text}");
    }
}
