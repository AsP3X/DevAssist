//! The `DevAssist` window: its assets, its theme, the baker that keeps its glass, and the window and shell
//! drawn with them.

pub mod app;
pub mod assets;
pub mod baker;
pub mod columns;
pub mod demo;
pub mod dialogs;
pub mod frost;
pub mod launch;
pub mod logger;
pub mod overlay;
pub mod repos_screen;
pub mod repos_view;
pub mod screenshot;
pub mod session;
pub mod shell;
pub mod table;
pub mod theme;
pub mod widgets;

#[cfg(test)]
mod contrast;
#[cfg(test)]
mod picture;
