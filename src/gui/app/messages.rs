use gpui::{
    div, prelude::*, px, AnyElement, Context, IntoElement, SharedString,
    StatefulInteractiveElement, Styled,
};
use gpui_component::text::markdown;

use super::ChatApp;
use crate::gui::components::{
    collapsible_display, current_spinner_frame, scrollbar, CollapsibleProps,
};
use crate::gui::state::{MessageRole, MessageSection};

impl ChatApp {
    pub(super) fn render_messages(&self, cx: &Context<Self>) -> impl IntoElement {
        let messages = self.conversation.messages.clone();
        let theme = self.theme.clone();
        let has_messages = !messages.is_empty();

        div()
            .id("messages-container")
            .flex()
            .flex_1()
            .w_full()
            .min_h(px(0.))
            .overflow_hidden()
            .child(
                div()
                    .id("messages-scroll")
                    .flex_1()
                    .w_full()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .track_scroll(&self.messages_scroll_handle)
                    .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                        // Check if user is at the bottom (within threshold)
                        let ratio =
                            crate::gui::components::scroll_ratio(&this.messages_scroll_handle);
                        // If ratio >= 0.98, user is at bottom, enable auto-scroll
                        // If ratio < 0.98, user scrolled away, disable auto-scroll
                        this.user_scrolled_away = ratio < 0.98;
                        cx.notify();
                    }))
                    .p(px(16.))
                    .when(!has_messages, |d| {
                        d.flex().items_center().justify_center().child(
                            div()
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap(px(12.))
                                .child(div().text_size(px(56.)).child("🍲"))
                                .child(
                                    div()
                                        .text_size(px(20.))
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(theme.text)
                                        .child("Welcome to Stockpot"),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .text_color(theme.text_muted)
                                        .child("Your AI-powered coding assistant"),
                                )
                                .child(
                                    div()
                                        .mt(px(16.))
                                        .text_size(px(13.))
                                        .text_color(theme.text_muted)
                                        .child("Type a message below to get started"),
                                )
                                .child(
                                    div()
                                        .mt(px(8.))
                                        .text_size(px(12.))
                                        .text_color(theme.text_muted)
                                        .child("📁 Drag and drop files here to share them"),
                                ),
                        )
                    })
                    .when(has_messages, |d| {
                        d.child(div().flex().flex_col().w_full().gap(px(16.)).children(
                            messages.into_iter().enumerate().map(|(idx, msg)| {
                                let is_user = msg.role == MessageRole::User;
                                let bubble_bg = if is_user {
                                    theme.user_bubble
                                } else {
                                    theme.assistant_bubble
                                };
                                let is_streaming = msg.is_streaming;

                                div()
                                    .id(SharedString::from(format!("msg-{}", idx)))
                                    .flex()
                                    .flex_col()
                                    .w_full()
                                    .when(is_user, |d| d.items_end())
                                    .when(!is_user, |d| d.items_start())
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(theme.text_muted)
                                            .mb(px(4.))
                                            .child(if is_user { "You" } else { "Assistant" }),
                                    )
                                    .child(
                                        div()
                                            .p(px(12.))
                                            .rounded(px(8.))
                                            .bg(bubble_bg)
                                            .text_color(theme.text)
                                            .overflow_hidden()
                                            .min_w_0()
                                            // User messages: constrained width, Assistant: full width
                                            .when(is_user, |d| d.max_w(px(600.)))
                                            .when(!is_user, |d| d.w_full().min_w_0())
                                            // Render sections if available, otherwise fall back to content
                                            .children(self.render_message_content(
                                                &msg.sections,
                                                &msg.content,
                                                idx,
                                                &theme,
                                                cx,
                                            ))
                                            .when(is_streaming, |d: gpui::Div| {
                                                d.child(
                                                    div()
                                                        .ml(px(2.))
                                                        .text_color(theme.accent)
                                                        .child(current_spinner_frame()),
                                                )
                                            }),
                                    )
                            }),
                        ))
                    }),
            )
            .child(scrollbar(
                self.messages_scroll_handle.clone(),
                self.messages_scrollbar_drag.clone(),
                theme.clone(),
            ))
    }

    /// Render the content of a message, handling sections or falling back to raw content.
    ///
    /// When a message has structured sections, each section is rendered appropriately:
    /// - Text sections render as markdown
    /// - NestedAgent sections render as collapsible containers
    ///
    /// If no sections exist (legacy messages), the raw content is rendered as markdown.
    fn render_message_content(
        &self,
        sections: &[MessageSection],
        content: &str,
        msg_idx: usize,
        theme: &crate::gui::theme::Theme,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        // If we have sections, render them
        if !sections.is_empty() {
            sections
                .iter()
                .enumerate()
                .map(|(sec_idx, section)| self.render_section(section, msg_idx, sec_idx, theme, cx))
                .collect()
        } else {
            // Legacy: render content directly as markdown
            // Clone to owned String for markdown renderer's 'static requirement
            let owned_content = content.to_string();
            vec![div()
                .w_full()
                .overflow_x_hidden()
                .child(markdown(&owned_content).selectable(true))
                .into_any_element()]
        }
    }

    /// Render a single message section.
    fn render_section(
        &self,
        section: &MessageSection,
        msg_idx: usize,
        sec_idx: usize,
        theme: &crate::gui::theme::Theme,
        cx: &Context<Self>,
    ) -> AnyElement {
        match section {
            MessageSection::Text(text) => {
                // Text sections render as markdown
                div()
                    .id(SharedString::from(format!(
                        "msg-{}-sec-{}",
                        msg_idx, sec_idx
                    )))
                    .w_full()
                    .overflow_x_hidden()
                    .child(markdown(text).selectable(true))
                    .into_any_element()
            }
            MessageSection::NestedAgent(agent_section) => {
                // Nested agent sections render as collapsible
                self.render_agent_section(agent_section, msg_idx, sec_idx, theme, cx)
            }
        }
    }

    /// Render a nested agent section as a collapsible container.
    fn render_agent_section(
        &self,
        agent_section: &crate::gui::state::AgentSection,
        msg_idx: usize,
        sec_idx: usize,
        theme: &crate::gui::theme::Theme,
        cx: &Context<Self>,
    ) -> AnyElement {
        let section_id_for_click = agent_section.id.clone();

        // Build collapsible props from theme and agent section state
        let props = CollapsibleProps::with_theme(theme)
            .id(format!("msg-{}-agent-{}", msg_idx, sec_idx))
            .title(agent_section.display_name.clone())
            .icon("🤖")
            .collapsed(agent_section.is_collapsed)
            .loading(!agent_section.is_complete);

        // Render the agent content as markdown
        let content = div()
            .w_full()
            .overflow_x_hidden()
            .child(markdown(&agent_section.content).selectable(true));

        // Create the collapsible in display-only mode (no internal click handler)
        let collapsible_element = collapsible_display(props, content);

        // Wrap in a clickable container that handles toggle via cx.listener()
        div()
            .id(SharedString::from(format!(
                "msg-{}-sec-{}-container",
                msg_idx, sec_idx
            )))
            .w_full()
            .my(px(8.)) // Vertical margin for visual separation
            .on_click(cx.listener(move |this, _, _, cx| {
                this.conversation
                    .toggle_section_collapsed(&section_id_for_click);
                cx.notify();
            }))
            .child(collapsible_element)
            .into_any_element()
    }
}
