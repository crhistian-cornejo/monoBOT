//! Where a dictation started by the global shortcut goes: the composer the
//! user is typing in, or the app in front, and the router that sends each
//! of the shortcut's presses and releases to the one that took its start.
use super::hotkey::{Action, KeyEvent, PressMachine};

/// Who receives the dictated text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The focused monoBOT composer, at its caret, as its own mic does.
    Composer,
    /// The app in front: the notch shows the dictation and the text is
    /// pasted where its caret was.
    System,
}

/// A monoBOT window in front with a composer focused dictates into that
/// composer; anything else (another app, a monoBOT window with no composer
/// focused, only the notch) dictates into the system.
pub fn route(monobot_window_frontmost: bool, focused_composer: bool) -> Target {
    if monobot_window_frontmost && focused_composer {
        Target::Composer
    } else {
        Target::System
    }
}

/// What the app knows when the shortcut goes down.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scene {
    /// A monoBOT window is the one in front.
    pub frontmost: bool,
    /// That window's focus is in a composer.
    pub focused_composer: bool,
    /// A composer the shortcut started is still listening (or opening its
    /// microphone), wherever the focus went since.
    pub composer_listening: bool,
    /// What has the focus in that window has its own binding for these
    /// keys (the code editor's Ctrl+Space completion, say).
    pub window_takes_key: bool,
}

/// Where one of the shortcut's events goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// The composer's own shortcut press: its tap / hold rule decides.
    ComposerDown,
    /// The key came up after a press the composer took.
    ComposerUp,
    /// The system dictation starts, stops or is dropped.
    System(Action),
    /// The keys go on to the focused monoBOT element that binds them, as if
    /// no hook had taken them.
    Window,
}

/// Sends each press of the global shortcut to the composer or the system
/// dictation, and its release to the same one. A system dictation that
/// listens takes the next press wherever the focus is, and a composer that
/// listens keeps its own, so a tap that started one is the tap that ends it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Router {
    system: PressMachine,
    pressed: Option<Target>,
}

impl Router {
    /// The system dictation is on (its press rule has started it).
    pub fn system_listening(&self) -> bool {
        self.system.recording()
    }

    /// The system dictation ended by itself (an error, the microphone went
    /// away, the model is missing): the next press starts a new one.
    pub fn system_ended(&mut self) {
        self.system.reset();
    }

    pub fn event(&mut self, event: KeyEvent, now_ms: u64, scene: Scene) -> Option<Delivery> {
        match event {
            KeyEvent::Press => {
                let target = if self.system.recording() {
                    Target::System
                } else if scene.composer_listening {
                    Target::Composer
                } else if scene.frontmost && !scene.focused_composer && scene.window_takes_key {
                    self.pressed = None;
                    return Some(Delivery::Window);
                } else {
                    route(scene.frontmost, scene.focused_composer)
                };
                self.pressed = Some(target);
                match target {
                    Target::Composer => Some(Delivery::ComposerDown),
                    Target::System => self.system.event(event, now_ms).map(Delivery::System),
                }
            }
            KeyEvent::Release => match self.pressed.take() {
                Some(Target::Composer) => Some(Delivery::ComposerUp),
                _ => self.system.event(event, now_ms).map(Delivery::System),
            },
            KeyEvent::Escape => self.system.event(event, now_ms).map(Delivery::System),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ELSEWHERE: Scene = Scene {
        frontmost: false,
        focused_composer: false,
        composer_listening: false,
        window_takes_key: false,
    };
    const COMPOSER: Scene = Scene {
        frontmost: true,
        focused_composer: true,
        composer_listening: false,
        window_takes_key: false,
    };

    #[test]
    fn a_monobot_element_that_binds_the_keys_keeps_them() {
        let mut router = Router::default();
        let editor = Scene {
            frontmost: true,
            window_takes_key: true,
            ..ELSEWHERE
        };
        assert_eq!(
            router.event(KeyEvent::Press, 0, editor),
            Some(Delivery::Window)
        );
        assert_eq!(router.event(KeyEvent::Release, 50, editor), None);
        assert!(!router.system_listening());
        // A monoBOT window in front with nothing that binds them dictates
        // into the system, so it pastes into that window.
        let plain = Scene {
            frontmost: true,
            ..ELSEWHERE
        };
        assert_eq!(
            router.event(KeyEvent::Press, 100, plain),
            Some(Delivery::System(Action::Start))
        );
        // Listening, the next press stops it even in the editor.
        router.event(KeyEvent::Release, 150, plain);
        assert_eq!(
            router.event(KeyEvent::Press, 900, editor),
            Some(Delivery::System(Action::Stop))
        );
    }

    #[test]
    fn another_app_in_front_gets_a_system_dictation() {
        let mut router = Router::default();
        assert_eq!(
            router.event(KeyEvent::Press, 0, ELSEWHERE),
            Some(Delivery::System(Action::Start))
        );
        // A tap: the release keeps it listening.
        assert_eq!(router.event(KeyEvent::Release, 100, ELSEWHERE), None);
        assert!(router.system_listening());
        // The next press stops it even with a composer in front now.
        assert_eq!(
            router.event(KeyEvent::Press, 2_000, COMPOSER),
            Some(Delivery::System(Action::Stop))
        );
        assert_eq!(router.event(KeyEvent::Release, 2_050, COMPOSER), None);
        assert!(!router.system_listening());
    }

    #[test]
    fn a_hold_elsewhere_is_push_to_talk_and_escape_cancels() {
        let mut router = Router::default();
        router.event(KeyEvent::Press, 0, ELSEWHERE);
        assert_eq!(
            router.event(KeyEvent::Release, 900, ELSEWHERE),
            Some(Delivery::System(Action::Stop))
        );
        router.event(KeyEvent::Press, 1_000, ELSEWHERE);
        router.event(KeyEvent::Release, 1_050, ELSEWHERE);
        assert_eq!(
            router.event(KeyEvent::Escape, 1_500, ELSEWHERE),
            Some(Delivery::System(Action::Cancel))
        );
        // With nothing listening, Escape is the app's.
        assert_eq!(router.event(KeyEvent::Escape, 1_600, ELSEWHERE), None);
    }

    #[test]
    fn a_focused_composer_gets_the_press_and_its_release() {
        let mut router = Router::default();
        assert_eq!(
            router.event(KeyEvent::Press, 0, COMPOSER),
            Some(Delivery::ComposerDown)
        );
        assert_eq!(
            router.event(KeyEvent::Release, 80, ELSEWHERE),
            Some(Delivery::ComposerUp)
        );
        assert!(!router.system_listening());
        // The composer still listens after a tap: the next press is its own,
        // even from another app.
        let listening = Scene {
            composer_listening: true,
            ..ELSEWHERE
        };
        assert_eq!(
            router.event(KeyEvent::Press, 3_000, listening),
            Some(Delivery::ComposerDown)
        );
        assert_eq!(
            router.event(KeyEvent::Release, 3_050, listening),
            Some(Delivery::ComposerUp)
        );
        // Escape is never taken for a composer: its own window handles it.
        assert_eq!(router.event(KeyEvent::Escape, 3_100, listening), None);
    }

    #[test]
    fn a_dictation_that_ended_by_itself_starts_again_on_the_next_press() {
        let mut router = Router::default();
        router.event(KeyEvent::Press, 0, ELSEWHERE);
        router.event(KeyEvent::Release, 50, ELSEWHERE);
        router.system_ended();
        assert!(!router.system_listening());
        assert_eq!(
            router.event(KeyEvent::Press, 500, ELSEWHERE),
            Some(Delivery::System(Action::Start))
        );
    }

    #[test]
    fn only_a_focused_composer_in_front_takes_the_dictation() {
        assert_eq!(route(true, true), Target::Composer);
        assert_eq!(route(true, false), Target::System);
        assert_eq!(route(false, false), Target::System);
        // A composer that kept focus in a window behind another app does not
        // receive it.
        assert_eq!(route(false, true), Target::System);
    }
}
