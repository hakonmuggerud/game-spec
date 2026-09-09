//! `ui.js` `toast` / `flushToasts` / `updateToast` — the one-at-a-time message line above the HUD.
//!
//! The queue itself is the shared [`crate::resources::Toasts`] resource (nothing else writes it);
//! [`ToastState`] is the display half, kept pure so the JS timing can be tested without a window.

use std::collections::VecDeque;

/// `ui.js` `ui.toastT` / the `#toast` element's text and opacity.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ToastState {
    /// `ui.toastT` — positive while a toast is up, then counts down through the 0.35 s gap.
    pub t: f32,
    /// `#toast.textContent` — kept after the fade-out, as the DOM does; only a flush clears it.
    pub text: String,
    /// `#toast.style.opacity === '1'`.
    pub visible: bool,
}

/// `ui.js:toast(msg)` — the volume/mute toasts are dropped while a list menu is open, because the Sound
/// panel already shows the level (`/^(Volume \d+%|Sound (on|off))$/`).
pub fn suppressed_while_menu_open(msg: &str) -> bool {
    if let Some(rest) = msg.strip_prefix("Volume ") {
        return rest
            .strip_suffix('%')
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    }
    msg == "Sound on" || msg == "Sound off"
}

impl ToastState {
    /// `ui.js:flushToasts()` — drop the backlog and clear the line at once (`title`, `saveReset`).
    pub fn flush(&mut self, queue: &mut VecDeque<String>) {
        queue.clear();
        self.t = 0.0;
        self.text.clear();
        self.visible = false;
    }

    /// `ui.js:updateToast(dt)`. `toast_t` is `CFG.toastT` (2.5 s).
    pub fn update(&mut self, dt: f32, queue: &mut VecDeque<String>, toast_t: f32) {
        if self.t > 0.0 {
            self.t -= dt;
            if self.t <= 0.0 {
                self.visible = false;
            } else {
                return;
            }
        }
        if self.t <= -0.35 || (self.t <= 0.0 && self.text.is_empty()) {
            if let Some(next) = queue.pop_front() {
                self.text = next;
                self.visible = true;
                self.t = toast_t;
            }
        } else if self.t <= 0.0 {
            // the short gap between two toasts
            self.t -= dt;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(items: &[&str]) -> VecDeque<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_first_toast_shows_at_once_and_holds_for_toast_t() {
        let mut s = ToastState::default();
        let mut queue = q(&["Banked 3 flasks"]);
        s.update(1.0 / 60.0, &mut queue, 2.5);
        assert_eq!(s.text, "Banked 3 flasks");
        assert!(s.visible);
        assert!(queue.is_empty());
        // still up just before 2.5 s
        for _ in 0..148 {
            s.update(1.0 / 60.0, &mut queue, 2.5);
        }
        assert!(s.visible);
        for _ in 0..3 {
            s.update(1.0 / 60.0, &mut queue, 2.5);
        }
        assert!(!s.visible, "hidden after CFG.toastT");
        assert_eq!(
            s.text, "Banked 3 flasks",
            "the text stays until it is replaced"
        );
    }

    #[test]
    fn a_second_toast_waits_out_the_gap() {
        let mut s = ToastState::default();
        let mut queue = q(&["one", "two"]);
        let dt = 1.0 / 60.0;
        s.update(dt, &mut queue, 2.5);
        assert_eq!(s.text, "one");
        // run out the 2.5 s
        for _ in 0..151 {
            s.update(dt, &mut queue, 2.5);
        }
        assert!(!s.visible);
        assert_eq!(s.text, "one", "the gap has not elapsed yet");
        // 0.35 s of gap, then the next one
        for _ in 0..22 {
            s.update(dt, &mut queue, 2.5);
        }
        assert_eq!(s.text, "two");
        assert!(s.visible);
    }

    #[test]
    fn flush_drops_the_backlog() {
        let mut s = ToastState::default();
        let mut queue = q(&["one", "two", "three"]);
        s.update(1.0 / 60.0, &mut queue, 2.5);
        s.flush(&mut queue);
        assert!(queue.is_empty());
        assert_eq!(s.text, "");
        assert!(!s.visible);
        // a toast emitted after the flush ("Left below: …") still shows immediately
        queue.push_back("Left below: 1 relic".to_string());
        s.update(1.0 / 60.0, &mut queue, 2.5);
        assert_eq!(s.text, "Left below: 1 relic");
    }

    #[test]
    fn volume_toasts_are_the_only_ones_suppressed() {
        assert!(suppressed_while_menu_open("Volume 60%"));
        assert!(suppressed_while_menu_open("Volume 0%"));
        assert!(suppressed_while_menu_open("Sound off"));
        assert!(suppressed_while_menu_open("Sound on"));
        assert!(!suppressed_while_menu_open("Volume %"));
        assert!(!suppressed_while_menu_open("Volume loud%"));
        assert!(!suppressed_while_menu_open("Save wiped."));
        assert!(!suppressed_while_menu_open("Sound"));
    }
}
