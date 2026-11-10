//! What the notch's player takes from Spotify's own Web API
//! (`monocode_core::notch::spotify`): "Playing next" under the player (each
//! track with its cover, a click away from playing), where
//! the title and each artist lead in Spotify, whether the track is among the
//! Liked Songs, and how shuffle stands. The system's media session tells
//! what plays, never any of that. The user signs in once from Notch
//! settings › Music with their own Spotify app's Client ID; the token stays
//! in the system keychain. Every request runs off the interface thread, at
//! most once per track and once more when the panel opens.
use super::*;
use monocode_core::notch::media::Track;
use monocode_core::notch::spotify::{self, Next};
use std::sync::{Arc, Mutex};

/// One waiting track's row: its cover, and its title over its artist.
const NEXT_ROW: f32 = 40.;
/// The cover's side in a row.
const NEXT_ART: f32 = 30.;
/// The line that says where "Playing next" comes from, before the sign-in.
const HINT: f32 = 28.;
/// How long a shuffle just asked for is shown before the player's own word
/// on it is: the system's controls take a moment to follow.
const WISH: Duration = Duration::from_secs(2);
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
    pub(super) connected: bool,
    /// The sign-in under way.
    connecting: Option<Task<()>>,
    /// What the sign-in or the last request has to say.
    status: Option<String>,
    session: Option<Arc<Mutex<spotify::Session>>>,
    pub(super) next: Vec<Next>,
    /// The track as Spotify itself knows it: its address, its artists'
    /// and how shuffle stands.
    now: Option<spotify::Now>,
    /// Whether that track is among the Liked Songs, once Spotify said.
    liked: Option<bool>,
    /// The sign-in is from before the Liked Songs were asked for.
    reconnect: bool,
    /// The track the queue was last read for, and when.
    read: Option<(Track, Instant)>,
    fetch: Option<Task<()>>,
    /// A like or a shuffle on its way to Spotify.
    change: Option<Task<()>>,
    /// The shuffle just asked for, shown until the player confirms it.
    wish: Option<bool>,
    wish_over: Option<Task<()>>,
    /// The listed tracks' covers by their address, each decoded once and
    /// kept while its track is listed.
    art: std::collections::HashMap<String, Arc<gpui::Image>>,
    art_fetch: Option<Task<()>>,
    /// The listed track chosen to play, on its way.
    skip: Option<Task<()>>,
}

/// What the player's heart can do now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Like {
    /// Nothing is signed in, the sign-in cannot save songs, or Spotify
    /// refused the last read: the heart leads to the sign-in.
    Connect,
    /// Spotify has not said yet, or what plays cannot be liked (an episode).
    Unknown,
    /// Among the Liked Songs, or not: the heart changes it.
    Known(bool),
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
        self.queue.next.clear();
        self.want_art(cx);
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
            if self.queue.read.take().is_some() || !self.queue.next.is_empty() {
                self.queue.next.clear();
                self.queue.now = None;
                self.queue.liked = None;
                self.want_art(cx);
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
        // Another track: what was known was about the one before.
        if self
            .queue
            .read
            .as_ref()
            .is_some_and(|(read, _)| *read != track)
        {
            self.queue.now = None;
            self.queue.liked = None;
        }
        let title = track.title.clone();
        self.queue.read = Some((track, Instant::now()));
        // Tests never touch the keychain or the network.
        if cfg!(test) {
            return;
        }
        let session = self.spotify_session();
        self.queue.fetch = Some(cx.spawn(async move |this, cx| {
            // Spotify's answer may still be about the track before: once
            // more, a little later, and then it is shown as it came.
            for attempt in 1..=2 {
                cx.background_executor().timer(SETTLE * attempt).await;
                let session = session.clone();
                let snapshot = cx
                    .background_executor()
                    .spawn(async move {
                        session
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .snapshot()
                    })
                    .await;
                let behind = snapshot.now.as_ref().is_some_and(|now| !now.is(&title));
                if behind && attempt == 1 {
                    continue;
                }
                let _ = this.update(cx, |this, cx| this.set_snapshot(snapshot, cx));
                break;
            }
        }));
    }

    fn spotify_session(&mut self) -> Arc<Mutex<spotify::Session>> {
        let client = self.look.spotify_client.clone();
        self.queue
            .session
            .get_or_insert_with(|| Arc::new(Mutex::new(spotify::Session::new(&client))))
            .clone()
    }

    /// Takes what Spotify said about the track that plays.
    pub(crate) fn set_snapshot(&mut self, snapshot: spotify::Snapshot, cx: &mut Context<Self>) {
        self.queue.status = snapshot.problem;
        self.queue.reconnect = snapshot.reconnect;
        self.queue.now = snapshot.now;
        self.queue.liked = snapshot.liked;
        self.queue.next = snapshot.next;
        self.want_art(cx);
        cx.notify();
    }

    /// Lets go of the covers of what is no longer listed (the picture and
    /// its texture) and asks for the ones still missing, one after another
    /// off the interface thread; each shows as it arrives.
    fn want_art(&mut self, cx: &mut Context<Self>) {
        let mut listed: Vec<String> = Vec::new();
        for address in self.queue.next.iter().filter_map(|next| next.art.clone()) {
            if !listed.contains(&address) {
                listed.push(address);
            }
        }
        let gone: Vec<String> = self
            .queue
            .art
            .keys()
            .filter(|address| !listed.contains(address))
            .cloned()
            .collect();
        for address in gone {
            if let Some(image) = self.queue.art.remove(&address) {
                gpui::ImageSource::Image(image).evict(cx);
            }
        }
        listed.retain(|address| !self.queue.art.contains_key(address));
        // Tests never touch the network.
        if listed.is_empty() || cfg!(test) {
            return;
        }
        self.queue.art_fetch = Some(cx.spawn(async move |this, cx| {
            for address in listed {
                let asked = address.clone();
                let bytes = cx
                    .background_executor()
                    .spawn(async move { spotify::fetch_art(&asked) })
                    .await;
                // Offline or refused: the row keeps its placeholder.
                let Ok(bytes) = bytes else {
                    continue;
                };
                let kept = this.update(cx, |this, cx| {
                    let listed = this
                        .queue
                        .next
                        .iter()
                        .any(|next| next.art.as_deref() == Some(address.as_str()));
                    if listed {
                        this.queue.art.insert(address, media::picture(&bytes));
                        cx.notify();
                    }
                });
                if kept.is_err() {
                    break;
                }
            }
        }));
    }

    /// Plays the listed track in place `index`. The ones before it are
    /// skipped, as choosing a song in Spotify's own queue does, and the list
    /// shows that at once. Spotify is asked where the track is now, since
    /// its queue may have changed since it was listed, and the player is
    /// told "next" that many times: the system's own command, so it works
    /// wherever previous and next do.
    pub(super) fn play_from_queue(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.queue.next.len() {
            return;
        }
        let Some(chosen) = self.queue.next.drain(..=index).last() else {
            return;
        };
        self.want_art(cx);
        cx.notify();
        // Tests never touch the keychain or the network.
        let session = (!cfg!(test)).then(|| self.spotify_session());
        self.queue.skip = Some(cx.spawn(async move |this, cx| {
            let now = match session {
                Some(session) => {
                    cx.background_executor()
                        .spawn(async move {
                            session
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner())
                                .queue()
                                // Offline or refused: its place as it was shown.
                                .unwrap_or_default()
                        })
                        .await
                }
                None => Vec::new(),
            };
            let presses = spotify::skips_to(&now, &chosen.uri, index);
            let _ = this.update(cx, |this, cx| {
                for _ in 0..presses {
                    this.send_media(platform::MediaCommand::Next, cx);
                }
            });
        }));
    }

    /// The track that plays, when Spotify plays it.
    fn spotify_track(&self) -> Option<&Track> {
        self.media
            .as_ref()
            .map(|media| &media.track)
            .filter(|track| media::Source::of(track) == media::Source::Spotify)
    }

    /// What Spotify said about the track that plays; none when it is not
    /// signed in, has not answered, or answered about another track.
    fn spotify_now(&self) -> Option<&spotify::Now> {
        let track = self.spotify_track()?;
        self.queue.now.as_ref().filter(|now| now.is(&track.title))
    }

    /// Where the playing track's title leads in Spotify.
    pub(super) fn track_link(&self) -> Option<String> {
        let track = self.spotify_track()?;
        Some(spotify::track_link(
            self.spotify_now(),
            &track.title,
            &track.artist,
        ))
    }

    /// The playing track's artists, each with where its name leads.
    pub(super) fn artist_links(&self) -> Vec<(String, String)> {
        self.spotify_track().map_or_else(Vec::new, |track| {
            spotify::artist_links(self.spotify_now(), &track.title, &track.artist)
        })
    }

    /// How shuffle stands where it can be changed from here; none where it
    /// cannot: the player does not say, or Spotify plays from somewhere
    /// without an order to shuffle (a radio, its own suggestions).
    pub(super) fn shuffle(&self) -> Option<bool> {
        let media = self.media.as_ref()?;
        let said = self.spotify_now();
        if said.is_some_and(|now| !now.can_shuffle) {
            return None;
        }
        let stands = media.shuffle.or(said.map(|now| now.shuffle))?;
        Some(self.queue.wish.unwrap_or(stands))
    }

    /// Turns shuffle the other way: through the system's controls where
    /// they report it, else through Spotify itself.
    pub(super) fn toggle_shuffle(&mut self, cx: &mut Context<Self>) {
        let Some(on) = self.shuffle() else {
            return;
        };
        let want = !on;
        self.queue.wish = Some(want);
        self.queue.wish_over = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(WISH).await;
            let _ = this.update(cx, |this, cx| {
                this.queue.wish = None;
                cx.notify();
            });
        }));
        if self
            .media
            .as_ref()
            .is_some_and(|media| media.shuffle.is_some())
        {
            self.send_media(platform::MediaCommand::Shuffle(want), cx);
            return;
        }
        cx.notify();
        // Tests never touch the keychain or the network.
        if cfg!(test) {
            return;
        }
        let session = self.spotify_session();
        self.queue.change = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    session
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .shuffle(want)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        if let Some(now) = this.queue.now.as_mut() {
                            now.shuffle = want;
                        }
                    }
                    Err(error) => {
                        this.queue.wish = None;
                        this.queue.status = Some(error);
                    }
                }
                cx.notify();
            });
        }));
    }

    /// What the heart can do for the track that plays.
    pub(super) fn like(&self) -> Like {
        if !self.queue_ready() || self.queue.reconnect {
            return Like::Connect;
        }
        match (self.spotify_now(), self.queue.liked) {
            (Some(now), Some(liked)) if now.likeable() => Like::Known(liked),
            // Spotify refused the last read (a sign-in that ran out, say):
            // Settings › Music says why.
            (None, _) if self.queue.status.is_some() => Like::Connect,
            _ => Like::Unknown,
        }
    }

    /// The heart: keeps the track among the Liked Songs or takes it out,
    /// shown at once and put back if Spotify refuses. Before a sign-in that
    /// can do it, it leads to the sign-in.
    pub(super) fn toggle_like(&mut self, cx: &mut Context<Self>) {
        let like = self.like();
        if like == Like::Connect {
            self.open_music_settings(cx);
            return;
        }
        let (Like::Known(liked), Some(now)) = (like, self.spotify_now()) else {
            return;
        };
        let (uri, want) = (now.uri.clone(), !liked);
        self.queue.liked = Some(want);
        cx.notify();
        // Tests never touch the keychain or the network.
        if cfg!(test) {
            return;
        }
        let session = self.spotify_session();
        self.queue.change = Some(cx.spawn(async move |this, cx| {
            let asked = uri.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    session
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .like(&asked, want)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    // Only while it is still the track that was asked about.
                    if this.queue.now.as_ref().is_some_and(|now| now.uri == uri) {
                        this.queue.liked = Some(!want);
                    }
                    this.queue.reconnect = error == spotify::NO_LIBRARY;
                    this.queue.status = Some(error);
                    cx.notify();
                }
            });
        }));
    }

    /// Notch settings › Music, where the sign-in is.
    fn open_music_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_page = settings::Page::Music;
        self.set_tab(Tab::Settings, cx);
    }

    /// Whether the line that leads to the sign-in shows: Spotify plays and
    /// nothing is signed in, so there is no queue to list.
    fn queue_hint(&self) -> bool {
        self.spotify_track().is_some() && !self.queue_ready()
    }

    /// The room "Playing next" takes under the player.
    pub(super) fn queue_height(&self) -> f32 {
        match self.queue.next.len() {
            0 if self.queue_hint() => HINT,
            0 => 0.,
            count => CAPTION + NEXT_ROW * count as f32,
        }
    }

    pub(super) fn queue_view(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.queue.next.is_empty() {
            if !self.queue_hint() {
                return div().into_any_element();
            }
            return div()
                .debug_selector(|| "notch-queue-hint".into())
                .h(px(HINT))
                .flex_shrink_0()
                .pt(px(6.))
                .px(px(2.))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(11.))
                        .text_color(ink(0.45))
                        .child("Connect Spotify to see what plays next"),
                )
                .child(
                    div().flex_shrink_0().child(
                        pill("notch-queue-connect", "Connect", false)
                            .on_click(cx.listener(|this, _, _, cx| this.open_music_settings(cx))),
                    ),
                )
                .into_any_element();
        }
        let mut list = div()
            .debug_selector(|| "notch-queue".into())
            .h(px(self.queue_height()))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .child(pages::caption("Playing next").pt(px(6.)));
        for (index, next) in self.queue.next.iter().enumerate() {
            let group = gpui::SharedString::from(format!("notch-next-{index}"));
            let cover = next
                .art
                .as_ref()
                .and_then(|address| self.queue.art.get(address))
                .cloned();
            // A row is a button: a click plays its track. Under the pointer
            // it lights up and shows the play mark at its end.
            list = list.child(
                div()
                    .id(("notch-next", index))
                    .group(group.clone())
                    .debug_selector(move || format!("notch-next-{index}"))
                    .role(Role::Button)
                    .aria_label(format!("Play {}", next.title))
                    .h(px(NEXT_ROW))
                    .px(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(2.))
                    .cursor_pointer()
                    .hover(|style| style.bg(ink(0.07)))
                    .active(|style| style.bg(ink(0.12)))
                    .on_click(cx.listener(move |this, _, _, cx| this.play_from_queue(index, cx)))
                    .child(
                        div()
                            .size(px(NEXT_ART))
                            .flex_shrink_0()
                            .overflow_hidden()
                            .rounded(px(2.))
                            .bg(ink(0.1))
                            .child(match cover {
                                Some(image) => gpui::img(image)
                                    .size(px(NEXT_ART))
                                    .object_fit(gpui::ObjectFit::Cover)
                                    .into_any_element(),
                                None => div()
                                    .size_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("media-note", 11.).text_color(ink(0.45)))
                                    .into_any_element(),
                            }),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(ink(0.94))
                                    .child(next.title.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(10.5))
                                    .line_height(px(14.))
                                    .text_color(ink(0.5))
                                    .child(next.artist.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .pr(px(4.))
                            .opacity(0.)
                            .group_hover(group, |style| style.opacity(1.))
                            .child(icon("media-play", 11.).text_color(ink(0.85))),
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
                                    .child("Spotify"),
                            )
                            .child(div().text_size(px(10.5)).text_color(ink(0.4)).child(
                                if connected && self.queue.reconnect {
                                    spotify::NO_LIBRARY
                                } else if connected {
                                    "Connected: Playing next, Liked Songs and Spotify's own links"
                                } else {
                                    "Playing next, Liked Songs and exact links in the player"
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
                        .on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
                            platform::take_keys(window)
                        })
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
