//! What differs between the platforms a harness run drives. The run engine (`run`) and the verb front end (`device::cli`) name no
//! platform: each driver crate module (`android`, `browser`) implements this once and passes itself in.
use crate::device::Device;
use crate::run_report::Facts;
use wf_kit::args::Parsed;
use std::rc::Rc;

pub trait Platform: Sync {
    /// The noun: `android` or `browser`.
    fn name(&self) -> &'static str;

    /// The platform's own options in the `test` usage line: the logpoint option text and the other options text.
    fn usage_options(&self) -> (&'static str, &'static str);

    /// Checks the options of a run that only this platform understands (or refuses), and sets what its driver reads.
    fn check_options(&self, p: &Parsed) -> Result<(), String>;

    /// What surrounds one run: Android's log capture and device lease. Dropped or finished when the run ends.
    fn begin_run(&self, stem: &str, started_ms: i64, p: &Parsed) -> Box<dyn RunScope>;

    /// The device a run drives, with the facts about it recorded in `f`.
    fn open_run_device(&self, serial: Option<String>, inputs: String, f: &mut Facts) -> Rc<dyn Device>;

    /// An option this platform cannot honour in a run, as the refusal's text.
    fn refuse_option(&self, p: &Parsed) -> Option<String>;

    /// Work between the device opening and the script starting (Android's density and app start).
    fn before_script(&self, device: &dyn Device, p: &Parsed, f: &mut Facts);

    /// The device a verb typed by hand drives: `--device`, else the only one. Exit 2 when several are open, 1 when none is.
    fn pick(&self, given: Option<&str>) -> Result<(Rc<dyn Device>, String), (i32, String)>;
}

/// What a platform keeps around a run, ended once the report's facts are complete.
pub trait RunScope {
    fn finish(self: Box<Self>, p: &Parsed, f: &mut Facts);
}

/// A platform with nothing around its runs.
pub struct NoScope;

impl RunScope for NoScope {
    fn finish(self: Box<Self>, _: &Parsed, _: &mut Facts) {}
}
