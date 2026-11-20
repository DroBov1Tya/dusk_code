//! Figures that keep their place while they change.

use gpui_kit::*;

/// Numbers that keep their width as they change, for counts that tick.
pub fn tabular() -> FontFeatures {
    FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)]))
}
