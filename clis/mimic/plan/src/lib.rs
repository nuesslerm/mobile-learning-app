//! The pure core of the capture marker: the grammar (`marker`) and the plan a marker makes over the sets of a checkout (`plan`). It reads the
//! world only through the `SetSource` port; `wf ci plan mimic` implements it over the disk and owns everything of CI (the commit, the outputs).
mod marker;
mod plan;

pub use marker::{parse, Action, Marker, DEFAULT_CHUNK_MINUTES, MAX_CHUNK_MINUTES, MAX_SETS};
pub use plan::{from_message, plan, MarkerPlan, Refusal, SetFacts, SetPlan, SetSource};
