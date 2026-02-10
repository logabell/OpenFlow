mod injector;
#[cfg(debug_assertions)]
pub mod logs;
pub mod tray;
pub mod uinput;
pub mod x11;

pub use injector::{
    OutputAction, OutputInjectionError, OutputInjector, PasteFailureKind, PasteShortcut,
};
