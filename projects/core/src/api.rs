//! Core API version shared with front ends.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiVersion {
    pub major: u16,
    pub minor: u16,
}

pub const CORE_API_VERSION: ApiVersion = ApiVersion { major: 0, minor: 1 };
