mod files_docs;
mod git_history;
mod git_staging;
mod security;
mod terminal;
mod ui_misc;
mod workspace;

use crate::app::{GitFleetApp, Message};
use iced::Task;

impl GitFleetApp {
    pub(crate) fn update(&mut self, message: Message) -> Task<Message> {
        let message = match self.try_update_workspace(message) {
            Ok(t) => return t,
            Err(m) => m,
        };
        let message = match self.try_update_git_history(message) {
            Ok(t) => return t,
            Err(m) => m,
        };
        let message = match self.try_update_git_staging(message) {
            Ok(t) => return t,
            Err(m) => m,
        };
        let message = match self.try_update_files_docs(message) {
            Ok(t) => return t,
            Err(m) => m,
        };
        let message = match self.try_update_security(message) {
            Ok(t) => return t,
            Err(m) => m,
        };
        let message = match self.try_update_terminal(message) {
            Ok(t) => return t,
            Err(m) => m,
        };
        match self.try_update_ui_misc(message) {
            Ok(t) => t,
            Err(unhandled) => {
                tracing::warn!("Unhandled Message variant: {unhandled:?}");
                Task::none()
            }
        }
    }
}
