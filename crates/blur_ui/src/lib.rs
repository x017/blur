//! Reusable native GPUI component library for Blur.
//!
//! Implements the Quiet Precision / Obsidian Precision design language:
//! hairline 1px borders, micro-tonal surface tiers, rectilinear 2-6px geometry,
//! monospaced keycap badges, razor-sharp tabs, and interactive text inputs.

pub mod components;
pub mod text_input;

pub use components::{
    badge, button, divider_h, divider_v, kbd, list_row, notification_toast, section_label,
    status_item, toggle_switch, BadgeTone, ButtonVariant, NotificationItem, NotificationLevel,
};
pub use text_input::{TextInput, TextInputEvent};
