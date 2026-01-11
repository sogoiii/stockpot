//! Smooth scroll animation for the messages list.
//!
//! This module provides smooth scroll interpolation to fix the "jumpy" feeling
//! when new streaming content arrives (especially with newlines that grow message height).
//!
//! The animation uses a "chase" approach where we continuously interpolate toward
//! the bottom position. This works well with streaming where the target keeps moving.

use gpui::{point, px, Pixels};

use super::ChatApp;

/// Animation speed for exponential smoothing (higher = faster)
/// At 3.0, animation reaches ~90% of target in ~750ms regardless of frame rate
const SCROLL_SMOOTH_SPEED: f32 = 3.0;

/// Minimum scroll distance to bother animating (in pixels)
/// Below this threshold, we snap to target immediately.
const MIN_SCROLL_THRESHOLD: Pixels = px(5.0);

/// If we're more than this many pixels from bottom, do an initial jump to get close
/// before starting smooth animation. This prevents long animation from top to bottom.
const INITIAL_JUMP_THRESHOLD: Pixels = px(800.0);

impl ChatApp {
    /// Start a smooth scroll animation to the bottom of the messages list.
    ///
    /// This uses a "chase" interpolation that continuously moves toward the
    /// bottom. It's designed for streaming where the target keeps moving.
    ///
    /// If we're very far from bottom (e.g., at the top of a new list), we do
    /// an initial jump to get close before starting the smooth animation.
    pub(super) fn start_smooth_scroll_to_bottom(&mut self) {
        // If already animating, just let it continue
        if self.scroll_animation_target.is_some() {
            return;
        }

        let current_offset = self.messages_list_state.scroll_px_offset_for_scrollbar();
        let max_offset = self.messages_list_state.max_offset_for_scrollbar();
        let target_y = -max_offset.height;
        let distance = (target_y - current_offset.y).abs();

        // If we're very far from bottom, jump most of the way first
        // This prevents a long slow scroll when the list first appears
        if distance > INITIAL_JUMP_THRESHOLD {
            // Jump to within ~100px of bottom, then animate the rest
            let jump_target = target_y + px(100.0);
            self.messages_list_state
                .set_offset_from_scrollbar(point(current_offset.x, jump_target));
        }

        // Mark that we want to animate to bottom
        self.scroll_animation_target = Some(point(px(0.), px(0.))); // Placeholder, real target computed in tick
        self.last_animation_tick = std::time::Instant::now();
    }

    /// Tick the scroll animation, interpolating toward the bottom.
    ///
    /// Uses delta-time exponential smoothing for frame-rate independent animation.
    /// This works well with streaming content where the target keeps moving.
    ///
    /// Returns `true` if we made a scroll adjustment, `false` if no adjustment needed.
    pub(super) fn tick_scroll_animation(&mut self) -> bool {
        // Skip if no animation requested or user has scrolled away
        if self.scroll_animation_target.is_none() || self.user_scrolled_away {
            self.scroll_animation_target = None;
            return false;
        }

        // Get current position and compute target (always the bottom)
        let current_offset = self.messages_list_state.scroll_px_offset_for_scrollbar();
        let max_offset = self.messages_list_state.max_offset_for_scrollbar();

        // Target is the bottom: y = -max_offset.height
        let target_y = -max_offset.height;
        let current_y = current_offset.y;

        // Calculate distance to target
        let distance = (target_y - current_y).abs();

        // If we're close enough, snap to target
        // BUT: Only stop animating if NOT streaming (target stops moving when streaming ends)
        if distance < MIN_SCROLL_THRESHOLD {
            self.messages_list_state
                .set_offset_from_scrollbar(point(current_offset.x, target_y));

            // Only stop animation when streaming is complete
            // During streaming, keep animation alive to chase the moving target
            if !self.is_generating {
                self.scroll_animation_target = None;
                return false;
            }
            // During streaming: stay snapped to bottom, keep animation alive
            return true;
        }

        // Delta-time based exponential smoothing (frame-rate independent)
        let now = std::time::Instant::now();
        let delta_secs = now.duration_since(self.last_animation_tick).as_secs_f32();
        self.last_animation_tick = now;

        // Clamp delta to prevent huge jumps after pauses (e.g., app was backgrounded)
        let delta_secs = delta_secs.min(0.1);

        // Exponential decay: factor approaches 1.0 over time, giving smooth deceleration
        // This produces identical animation speed on 60Hz, 120Hz, or variable refresh
        let factor = 1.0 - (-SCROLL_SMOOTH_SPEED * delta_secs).exp();
        let new_y = current_y + (target_y - current_y) * factor;

        self.messages_list_state
            .set_offset_from_scrollbar(point(current_offset.x, new_y));

        true // Animation still in progress
    }
}
