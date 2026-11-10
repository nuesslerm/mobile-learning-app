//! `hosted.pr`: pull and merge requests, runs and the repository's candidates for labels and reviewers. `pr.*` and `mr.*` are one set of
//! methods under two spellings (the provider a request names none gets differs), each answered by one executor action.
use super::{spec, Spec, GROUP_PR};
use agent_api_proto::Method;

macro_rules! both {
    ($(($pr:ident, $mr:ident, $name:literal, $action:expr)),* $(,)?) => {
        $(
            pub const $pr: Method = Method::new(GROUP_PR, concat!("pr.", $name));
            pub const $mr: Method = Method::new(GROUP_PR, concat!("mr.", $name));
        )*
        /// The `pr.*` methods, then the `mr.*` ones, in the contract's order.
        pub const METHODS: &[Method] = &[$($pr),*, $($mr),*];
        /// Each method with its action: the `pr.*` ones default to GitHub, the `mr.*` ones to GitLab.
        pub const SPECS: &[Spec] = &[$(spec($pr, &[$action], Some("github"))),*, $(spec($mr, &[$action], Some("gitlab"))),*];
    };
}

both! {
    (LIST, MR_LIST, "list", exec_hosted::PR_LIST),
    (VIEW, MR_VIEW, "view", exec_hosted::PR_VIEW),
    (DIFF, MR_DIFF, "diff", exec_hosted::PR_DIFF),
    (CHECKS, MR_CHECKS, "checks", exec_hosted::PR_CHECKS),
    (COMMENTS, MR_COMMENTS, "comments", exec_hosted::PR_COMMENTS),
    (REVIEWS, MR_REVIEWS, "reviews", exec_hosted::PR_REVIEWS),
    (THREADS, MR_THREADS, "threads", exec_hosted::PR_THREADS),
    (FILES, MR_FILES, "files", exec_hosted::PR_FILES),
    (CREATE, MR_CREATE, "create", exec_hosted::PR_CREATE),
    (EDIT, MR_EDIT, "edit", exec_hosted::PR_EDIT),
    (READY, MR_READY, "ready", exec_hosted::PR_READY),
    (COMMENT, MR_COMMENT, "comment", exec_hosted::PR_COMMENT),
    (REPLY, MR_REPLY, "reply", exec_hosted::PR_REPLY),
    (REVIEW, MR_REVIEW, "review", exec_hosted::PR_REVIEW),
    (REQUEST_REVIEWERS, MR_REQUEST_REVIEWERS, "requestReviewers", exec_hosted::PR_REQUEST_REVIEWERS),
    (SET_LABELS, MR_SET_LABELS, "setLabels", exec_hosted::PR_LABELS),
    (REACT, MR_REACT, "react", exec_hosted::PR_REACT),
    (RESOLVE_THREAD, MR_RESOLVE_THREAD, "resolveThread", exec_hosted::PR_RESOLVE_THREAD),
    (UPDATE_BRANCH, MR_UPDATE_BRANCH, "updateBranch", exec_hosted::PR_UPDATE_BRANCH),
    (MERGE, MR_MERGE, "merge", exec_hosted::PR_MERGE),
    (STACK, MR_STACK, "stack", exec_hosted::PR_STACK),
    (EDIT_COMMENT, MR_EDIT_COMMENT, "editComment", exec_hosted::PR_COMMENT_EDIT),
    (LABEL_CANDIDATES, MR_LABEL_CANDIDATES, "labelCandidates", exec_hosted::REPO_LABELS),
    (REVIEWER_CANDIDATES, MR_REVIEWER_CANDIDATES, "reviewerCandidates", exec_hosted::REPO_REVIEWERS),
    (RUN_LIST, MR_RUN_LIST, "runs", exec_hosted::RUN_LIST),
    (RUN_VIEW, MR_RUN_VIEW, "run", exec_hosted::RUN_VIEW),
}
