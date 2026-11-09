//! "Playing next" under the notch's player, from Spotify's own Web API
//! (`monocode_core::notch::spotify`): the system's media session tells what
//! plays, never what comes after. The user signs in once from Notch
//! settings › Music with their own Spotify app's Client ID; the token stays
//! in the system keychain. Every request runs off the interface thread, at
//! most once per track and once more when the panel opens.
use super::*;
use monocode_core::notch::media::Track;
use monocode_core::notch::spotify::{self, Next};
use std::sync::{Arc, Mutex};

/// One waiting track's row.
const NEXT_ROW: f32 = 22.;
/// The queue is not asked for again sooner than this.
const FRESH: Duration = Duration::from_secs(8);
/// Spotify's API learns of a new track a moment after the player does.
const SETTLE: Duration = Duration::from_millis(1200);
/// The room the fullest queue takes, for the window's fixed room.
pub(super) const TALLEST: f32 = CAPTION + NEXT_ROW * spotify::SHOWN as f32;
const DASHBOARD: &str = "https://developer.spotify.com/dashboard";

#[derive(Default)]
pub(super) struct Queue {
    /// A sign-in is kept in the keychain.
    connected: bool,
    /// The sign-in under way.
    connecting: Option<Task<()>>,
    /// What the sign-in or the last request has to say.
    status: Option<String>,
    session: Option<Arc<Mutex<spotify::Session>>>,
    pub(super) next: Vec<Next>,
    /// The track the queue was last read for, and when.
    read: Option<(Track, Instant)>,
    fetch: Option<Task<()>>,
}

impl Notch {
    /// Reads whether a sign-in is kept, off the interface thread.
    pub(super) fn start_queue(&mut self, cx: &mut Context<Self>) {
        // Tests never touch the keychain.
        if cfg!(test) {
            return;
        }
        let kept = cx
            .background_executor()
            .spawn(async { spotify::connected() });
        cx.spawn(async move |this, cx| {
            let connected = kept.await;
            let _ = this.update(cx, |this, cx| {
                this.queue.connected = connected;
                this.read_queue(true, cx);
            });
        })
        .detach();
    }

    fn queue_ready(&self) -> bool {
        self.queue.connected && !self.look.spotify_client.is_empty()
    }

    /// Signs in with the Client ID in the field: opens Spotify's page in the
    /// browser and waits for it to come back.
    fn connect_spotify(&mut self, cx: &mut Context<Self>) {
        let client = self.spotify_field.read(cx).content.trim().to_owned();
        if !spotify::valid_client(&client) {
            self.queue.status = Some("A Client ID is 32 letters and digits.".into());
            cx.notify();
            return;
        }
        self.change_look(|look| look.spotify_client = client.clone(), cx);
        self.queue.status = Some("Waiting for Spotify in your browser…".into());
        let begun = cx
            .background_executor()
            .spawn(async move { spotify::begin(&client) });
        self.queue.connecting = Some(cx.spawn(async move |this, cx| {
            let result = match begun.await {
                Ok(sign_in) => {
                    let url = sign_in.url.clone();
                    if this.update(cx, |_, cx| cx.open_url(&url)).is_err() {
                        return;
                    }
                    cx.background_executor()
                        .spawn(async move { sign_in.finish() })
                        .await
                }
                Err(error) => Err(error),
            };
            let _ = this.update(cx, |this, cx| {
                this.queue.connecting = None;
                match result {
                    Ok(()) => {
                        this.queue.connected = true;
                        this.queue.status = None;
                        this.queue.session = None;
                        this.read_queue(true, cx);
                    }
                    Err(error) => this.queue.status = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn disconnect_spotify(&mut self, cx: &mut Context<Self>) {
        self.queue = Queue::default();
        cx.background_executor()
            .spawn(async { spotify::disconnect() })
            .detach();
        cx.notify();
    }

    /// Asks Spotify what plays next, when Spotify is what plays and the
    /// answer kept is for another track or (with `again`) no longer fresh.
    pub(super) fn read_queue(&mut self, again: bool, cx: &mut Context<Self>) {
        let track = self
            .media
            .as_ref()
            .map(|media| media.track.clone())
            .filter(|track| media::Source::of(track) == media::Source::Spotify);
        let Some(track) = track.filter(|_| self.queue_ready()) else {
            if !self.queue.next.is_empty() {
                self.queue.next.clear();
                self.queue.read = None;
                cx.notify();
            }
            return;
        };
        let fresh = self
            .queue
            .read
            .as_ref()
            .is_some_and(|(read, at)| *read == track && (!again || at.elapsed() < FRESH));
        if fresh {
            return;
        }
        self.queue.read = Some((track, Instant::now()));
        let client = self.look.spotify_client.clone();
        let session = self
            .queue
            .session
            .get_or_insert_with(|| Arc::new(Mutex::new(spotify::Session::new(&client))))
            .clone();
        self.queue.fetch = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE).await;
            let result = cx
                .background_executor()
                .spawn(async move {
                    session
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .queue()
                })
                .await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(next) => {
                    this.queue.status = None;
                    this.set_queue(next, cx);
                }
                Err(error) => {
                    this.queue.status = Some(error);
                    this.set_queue(Vec::new(), cx);
                }
            });
        }));
    }

    pub(crate) fn set_queue(&mut self, next: Vec<Next>, cx: &mut Context<Self>) {
        if self.queue.next != next {
            self.queue.next = next;
            cx.notify();
        }
    }

    /// The room "Playing next" takes under the player.
    pub(super) fn queue_height(&self) -> f32 {
        match self.queue.next.len() {
            0 => 0.,
            count => CAPTION + NEXT_ROW * count as f32,
        }
    }

    pub(super) fn queue_view(&self) -> AnyElement {
        if self.queue.next.is_empty() {
            return div().into_any_element();
        }
        let mut list = div()
            .debug_selector(|| "notch-queue".into())
            .h(px(self.queue_height()))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .child(pages::caption("Playing next").pt(px(6.)));
        for (index, next) in self.queue.next.iter().enumerate() {
            list = list.child(
                div()
                    .h(px(NEXT_ROW))
                    .px(px(2.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .w(px(12.))
                            .flex_shrink_0()
                            .text_size(px(10.5))
                            .text_color(ink(0.35))
                            .child(format!("{}", index + 1)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(ink(0.88))
                            .child(next.title.clone()),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .max_w(px(170.))
                            .truncate()
                            .text_size(px(11.))
                            .text_color(ink(0.45))
                            .child(next.artist.clone()),
                    ),
            );
        }
        list.into_any_element()
    }

    /// Notch settings › Music: the sign-in for "Playing next".
    pub(super) fn music_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let connected = self.queue.connected;
        let connecting = self.queue.connecting.is_some();
        let small = |text: String| {
            div()
                .py(px(3.))
                .text_size(px(10.5))
                .line_height(px(14.))
                .text_color(ink(0.45))
                .child(text)
        };
        let action = if connected {
            pill("notch-spotify-disconnect", "Disconnect", false)
                .on_click(cx.listener(|this, _, _, cx| this.disconnect_spotify(cx)))
        } else if connecting {
            pill("notch-spotify-cancel", "Cancel", false).on_click(cx.listener(|this, _, _, cx| {
                this.queue.connecting = None;
                this.queue.status = None;
                cx.notify();
            }))
        } else {
            pill("notch-spotify-connect", "Connect", true)
                .on_click(cx.listener(|this, _, _, cx| this.connect_spotify(cx)))
        };
        let mut page = div()
            .debug_selector(|| "notch-music-page".into())
            .flex()
            .flex_col()
            .child(
                div()
                    .min_h(px(34.))
                    .py(px(3.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(ink(0.88))
                                    .child("Spotify queue"),
                            )
                            .child(div().text_size(px(10.5)).text_color(ink(0.4)).child(
                                if connected {
                                    "Connected: Playing next shows under the player"
                                } else {
                                    "What plays next, under the player"
                                },
                            )),
                    )
                    .child(div().flex_shrink_0().child(action)),
            );
        if !connected {
            page = page
                .child(
                    div()
                        .h(px(30.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .bg(background_base())
                        .border_1()
                        .border_color(ink(0.16))
                        .child(self.spotify_field.clone()),
                )
                .child(small(format!(
                    "Spotify asks each person for an app of their own: make one in its dashboard, add the redirect URI {} and paste the app's Client ID above.",
                    spotify::REDIRECT
                )))
                .child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(
                            pill("notch-spotify-dashboard", "Open dashboard", false)
                                .on_click(|_, _, cx| cx.open_url(DASHBOARD)),
                        )
                        .child(
                            pill("notch-spotify-redirect", "Copy redirect URI", false).on_click(
                                |_, _, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        spotify::REDIRECT.into(),
                                    ))
                                },
                            ),
                        ),
                );
        }
        if let Some(status) = self.queue.status.clone() {
            page = page.child(
                small(status)
                    .debug_selector(|| "notch-spotify-status".into())
                    .text_color(accent_color()),
            );
        }
        page.into_any_element()
    }
}
