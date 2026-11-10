//! What the notch knows that the menu bar's live items show too: the agents
//! at work, as the wings see them, and what plays.
use super::*;
use monocode_core::menu_bar;

impl Notch {
    pub(crate) fn live_agents(&self) -> menu_bar::Agents {
        menu_bar::Agents {
            working: self.rest.working,
            asking: self.rest.asking,
            word: self.rest.lead_text.clone(),
            steps: self.rest.lead_steps,
        }
    }

    pub(crate) fn live_music(&self) -> Option<menu_bar::Music> {
        let playing = self.media.as_ref()?;
        Some(menu_bar::Music {
            title: playing.track.title.clone(),
            artist: playing.track.artist.clone(),
            playing: playing.playing,
        })
    }

    /// The Windows tray's "Open Notch" opens the notch where the live item
    /// lives.
    #[cfg_attr(any(test, not(target_os = "windows")), allow(dead_code))]
    pub(crate) fn open_on(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.editing = false;
        self.set_open(true, cx);
        cx.notify();
    }
}
