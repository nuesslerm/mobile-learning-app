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
}
