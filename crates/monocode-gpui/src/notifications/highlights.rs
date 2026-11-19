//! What's new's highlights: each version's few changes worth a picture, as
//! cards with a screenshot (or a dithered banner when there is none), a
//! tag, a title and what changed underneath. The release's full notes follow
//! them in the dialog.

/// What kind of change a highlight is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tag {
    New,
    Improved,
    Fixed,
}

impl Tag {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Tag::New => "New",
            Tag::Improved => "Improved",
            Tag::Fixed => "Fixed",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Highlight {
    pub tag: Tag,
    pub title: &'static str,
    pub body: &'static str,
    /// The icon on the banner when there is no screenshot.
    pub icon: &'static str,
    /// A bundled screenshot's asset path (`whats-new/…`), shown fading into
    /// the card over a blurred copy of itself.
    pub image: Option<&'static str>,
}

const V0_0_8: [Highlight; 7] = [
    Highlight {
        tag: Tag::New,
        title: "Captains: one sidebar for every agent",
        body: "Your personal agents are now Captains, with their own chats, above your threads. Code and personal agents share one sidebar, and any provider can hand work to your monos as subagents or a swarm.",
        icon: "users",
        image: None,
    },
    Highlight {
        tag: Tag::New,
        title: "A compact model picker",
        body: "Favorites first, then every model grouped by who makes it, or by provider. Hover a model to set its effort on a dithered slider, turn on fast mode, or make it a favorite with ⌥1–⌥9.",
        icon: "sliders",
        image: None,
    },
    Highlight {
        tag: Tag::New,
        title: "Hand a thread to your crew",
        body: "From a thread's menu, Chief takes its latest request, splits it among your monos and sends one summary back. The right panel's Crew tab follows the run, and keeps its faces after it ends.",
        icon: "rocket",
        image: None,
    },
    Highlight {
        tag: Tag::Improved,
        title: "Dictation you can watch",
        body: "Words show as you say them. The speech model loads when monoBOT opens and stays ready, so stopping no longer waits on \"Finishing…\"; if it ever does, Esc gets you out.",
        icon: "mic-01",
        image: None,
    },
    Highlight {
        tag: Tag::Improved,
        title: "Corners your way, notch included",
        body: "Choose square to soft corners in Settings › Appearance, and separately for the notch, whose outline opens rounder or sharper to match. Menus are native on macOS and follow the system accent.",
        icon: "palette",
        image: None,
    },
    Highlight {
        tag: Tag::Improved,
        title: "Threads that show their time",
        body: "A working thread shows a live timer in the sidebar. Workstreams moved to their own page, from the bar at the foot of the sidebar.",
        icon: "clock",
        image: None,
    },
    Highlight {
        tag: Tag::Fixed,
        title: "Agents that reach the app",
        body: "Codex and OpenCode chats can now call Chief and swarms from their sandbox. A new request is no longer mistaken for an older Chief run, and deleting Chief's chat ends its run.",
        icon: "check-circle",
        image: None,
    },
];

const V0_0_9: [Highlight; 3] = [
    Highlight {
        tag: Tag::Fixed,
        title: "Captain chats read in order",
        body: "A message that reached the agent and then failed no longer shows twice, once with its error and again at the bottom as \"Not sent\". Copies saved that way are cleaned up when the chat opens.",
        icon: "message-multiple",
        image: None,
    },
    Highlight {
        tag: Tag::Improved,
        title: "Max moves the corner everywhere",
        body: "The model card's dithered corner moves for Max and every Ultra effort, wherever they sit in a model's list, and for a provider's own effort control.",
        icon: "sparkles",
        image: None,
    },
    Highlight {
        tag: Tag::Fixed,
        title: "Crews and Captains behave",
        body: "Captains made with their own instructions keep them, a chat whose Chief run ended can start a swarm again, deleting Chief's chat forgets its run, and Chief's crew shows under Monos in every project.",
        icon: "users",
        image: None,
    },
];

const V0_0_10: [Highlight; 4] = [
    Highlight {
        tag: Tag::New,
        title: "Gemini 4 Neon",
        body: "Google's new model leads Antigravity's list and is its default. Antigravity's per-effort models fold into one per family, so the effort slider picks the variant.",
        icon: "sparkles",
        image: None,
    },
    Highlight {
        tag: Tag::New,
        title: "Hand a task to one of your monos",
        body: "Any coding chat knows your code monos by name and specialty and can give one a task. The mono works in its own chat and memory, and its answer comes back to the chat.",
        icon: "rocket",
        image: None,
    },
    Highlight {
        tag: Tag::New,
        title: "Groups, and a Principal that leads",
        body: "Group chats have their own section under the Captains. The Principal reaches every project open in monoBOT: it reads code, follows sessions and hands parts to Captains and monos.",
        icon: "users",
        image: None,
    },
    Highlight {
        tag: Tag::Improved,
        title: "Monos are yours",
        body: "Only you create monos, in the sidebar, and they save what they're told to their memory. Chief and swarms use them when one fits and never make their own.",
        icon: "smile",
        image: None,
    },
];

/// The highlights of `version`, newest release first in the source.
pub(crate) fn of(version: &str) -> &'static [Highlight] {
    match version.trim().trim_start_matches('v') {
        "0.0.10" => &V0_0_10,
        "0.0.9" => &V0_0_9,
        "0.0.8" => &V0_0_8,
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::AssetSource;

    #[test]
    fn each_release_names_its_highlights_with_icons_that_exist() {
        for highlight in of("v0.0.8").iter().chain(of("0.0.9")).chain(of("0.0.10")) {
            assert!(!highlight.title.is_empty() && !highlight.body.is_empty());
            assert!(
                crate::assets::Assets
                    .load(highlight.icon)
                    .ok()
                    .flatten()
                    .is_some(),
                "{} is bundled",
                highlight.icon
            );
            if let Some(image) = highlight.image {
                assert!(crate::assets::Assets.load(image).ok().flatten().is_some());
            }
        }
        assert!(of("0.0.1").is_empty());
    }
}
