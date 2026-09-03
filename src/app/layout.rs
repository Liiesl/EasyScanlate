// Re-export canonical layout constants/types now owned by `easyscanlate-ui`.
// Kept as thin shim so existing `crate::app::layout::*` imports keep working
// while new code should `use easyscanlate_ui::layout::*` directly.
#[allow(unused_imports)]
pub use easyscanlate_ui::layout::*;
