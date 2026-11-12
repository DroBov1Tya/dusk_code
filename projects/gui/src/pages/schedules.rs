//! Scheduled tasks.

use gpui_kit::*;

use crate::workspace::{Center, Workspace};

pub fn render(_: &mut Workspace, _: &mut Window, _: &mut Context<Workspace>) -> Div {
    div()
}

impl Workspace {
    pub fn open_schedules(&mut self, cx: &mut Context<Self>) {
        self.center = Center::Schedules;
        cx.notify();
    }
}
