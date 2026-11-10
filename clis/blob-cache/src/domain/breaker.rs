/// Why a run went cold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Degraded {
    Unreachable,
    Credential,
    Budget,
}

impl Degraded {
    pub fn as_str(self) -> &'static str {
        match self {
            Degraded::Unreachable => "unreachable",
            Degraded::Credential => "credential",
            Degraded::Budget => "budget",
        }
    }
}

/// Opens on the first failure of a run and stays open: the store is not asked again, and the first reason is the one reported.
#[derive(Debug, Default)]
pub struct Breaker {
    open: Option<Degraded>,
}

impl Breaker {
    pub fn trip(&mut self, why: Degraded) {
        self.open.get_or_insert(why);
    }

    pub fn open(&self) -> Option<Degraded> {
        self.open
    }
}
