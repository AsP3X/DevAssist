//! The `DevAssist` window: its assets, its theme, the baker that keeps its glass, and the window and shell
//! drawn with them.

pub mod app;
pub mod assets;
pub mod baker;
pub mod demo;
pub mod frost;
pub mod launch;
pub mod logger;
pub mod repos_view;
pub mod screenshot;
pub mod session;
pub mod shell;
pub mod theme;

#[cfg(test)]
mod contrast;
