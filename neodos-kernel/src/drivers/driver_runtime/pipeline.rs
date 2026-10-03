//! Driver certification pipeline step tracking.

// ── Certification step tracking ──

/// Pipeline step that failed (0 = no failure / passed all)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PipelineStep {
    None = 0,
    Load = 1,
    Init = 2,
    Registration = 3,
    Binding = 4,
    Certification = 5,
    Unloading = 6,
}

impl PipelineStep {
    pub fn to_str(self) -> &'static str {
        match self {
            PipelineStep::None => "OK",
            PipelineStep::Load => "LOAD",
            PipelineStep::Init => "INIT",
            PipelineStep::Registration => "REGISTER",
            PipelineStep::Binding => "BIND",
            PipelineStep::Certification => "CERTIFY",
            PipelineStep::Unloading => "UNLOAD",
        }
    }
}
