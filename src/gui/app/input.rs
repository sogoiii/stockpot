use gpui::{div, prelude::*, px, rgb, Context, MouseButton, Styled};

use super::ChatApp;

impl ChatApp {
    pub(super) fn render_input(&self, cx: &Context<Self>) -> impl IntoElement {
        let is_generating = self.is_generating;
        let theme = self.theme.clone();

        div()
            .flex()
            .items_end()
            .gap(px(12.))
            .p(px(16.))
            .border_t_1()
            .border_color(self.theme.border)
            .bg(self.theme.panel_background)
            .child(self.text_input.clone())
            .child(
                div()
                    .id("send-btn")
                    .px(px(16.))
                    .py(px(10.))
                    .rounded(px(8.))
                    .bg(if is_generating {
                        theme.text_muted
                    } else {
                        theme.accent
                    })
                    .text_color(rgb(0xffffff))
                    .cursor_pointer()
                    .hover(|s| s.opacity(0.9))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _window, cx| {
                            if !this.is_generating {
                                this.send_message(cx);
                            }
                        }),
                    )
                    .child(if is_generating { "⏳" } else { "Send →" }),
            )
    }
}
