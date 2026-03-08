mod injector;
#[cfg(debug_assertions)]
pub mod logs;
pub mod tray;
pub mod uinput;
pub mod x11;

pub use injector::{
    synthetic_paste_active, OutputAction, OutputInjectionError, OutputInjector, PasteFailureKind,
    PasteShortcut,
};
