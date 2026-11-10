//! What plays, in the notch. Closed, while a song or a video plays: its
//! cover, its title gliding past and a small equalizer. Open, the player
//! leads Home: the cover with the app's mark, the title, the artist in the
//! accent, the Lyrics switch, a bar to drag through the track and the
//! buttons; the lyrics themselves play on the closed notch.
//! The track comes from the platform (`platform::watch_media`: the media
//! session on Windows, MediaRemote on macOS), the
//! synced lyrics from LRCLIB (`monocode_core::notch::media`), fetched off
//! the interface thread once per track.
use super::*;
use gpui::{MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, WeakEntity};
use monocode_core::notch::media::{self as media_model, Lyrics, Playing, Track};
use std::cell::Cell;
use std::sync::{Arc, OnceLock};

/// Each wing's room while something plays, with or without agents at work:
/// one width, so an agent starting under the music does not move it.
pub(super) const MEDIA_WING: u32 = 160;
/// The player at the top of Home.
pub(super) const PLAYER_HEIGHT: f32 = 112.;
/// How often the open player's line and progress catch up.
const TICK: Duration = Duration::from_millis(250);
/// The title's glide: points a second, the pause at its start, the gap
/// before it comes round again.
const GLIDE_SPEED: f32 = 28.;
const GLIDE_PAUSE: f32 = 1.6;
const GLIDE_GAP: f32 = 36.;

/// Keeps the scrubber's drag (0 to 1) between frames.
pub(super) type Scrub = Rc<Cell<Option<f32>>>;

/// The clock every glide and equalizer runs on, so they keep their phase
/// between frames.
fn since_start() -> f32 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f32()
}

/// The app that plays, for its mark on the cover.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Source {
    Spotify,
    Browser,
    Other,
}

impl Source {
    pub(super) fn of(track: &Track) -> Self {
        let app = track.app.to_lowercase();
        if app.contains("spotify") {
            Self::Spotify
        } else if [
            "chrome",
            "msedge",
            "edgemac",
            "firefox",
            "brave",
            "opera",
            "vivaldi",
            "safari",
            "thebrowser",
            "308046b0af4a39cb",
        ]
        .iter()
        .any(|browser| app.contains(browser))
        {
            Self::Browser
        } else {
            Self::Other
        }
    }
}

impl Notch {
    /// Starts watching the player (never in tests).
    pub(super) fn watch_media(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) {
            return;
        }
        let (sender, changes) = async_channel::unbounded();
        let Some(commands) = platform::watch_media(move |playing| {
            let _ = sender.try_send(playing);
        }) else {
            return;
        };
        self.media_commands = Some(commands);
        self.media_task = Some(cx.spawn(async move |this, cx| {
            while let Ok(playing) = changes.recv().await {
                if this
                    .update(cx, |this, cx| this.set_media(playing, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    pub(crate) fn set_media(&mut self, playing: Option<Playing>, cx: &mut Context<Self>) {
        if tracing() {
            eprintln!(
                "NOTCHTRACE media {:?} art {}",
                playing
                    .as_ref()
                    .map(|p| (p.track.label(), p.playing, p.position)),
                playing.as_ref().is_some_and(|p| p.art.is_some())
            );
        }
        // The cover is decoded once per track: GPUI keys its texture by the
        // image, so the same `Arc<Image>` draws every frame for free.
        match playing.as_ref().and_then(|playing| playing.art.clone()) {
            Some(bytes)
                if !self
                    .art
                    .as_ref()
                    .is_some_and(|(kept, _)| Arc::ptr_eq(kept, &bytes)) =>
            {
                let format = if bytes.starts_with(&[0xff, 0xd8]) {
                    gpui::ImageFormat::Jpeg
                } else if bytes.starts_with(b"RIFF") {
                    gpui::ImageFormat::Webp
                } else {
                    gpui::ImageFormat::Png
                };
                let image = Arc::new(gpui::Image::from_bytes(format, bytes.to_vec()));
                let before = self.art.replace((bytes, image));
                Self::forget_cover(before, cx);
            }
            Some(_) => {}
            None => {
                let before = self.art.take();
                Self::forget_cover(before, cx);
            }
        }
        let track = playing.as_ref().map(|playing| playing.track.clone());
        self.media = playing;
        let _ = track;
        self.want_lyrics(cx);
        self.read_queue(false, cx);
        // The open player's line and progress move a few times a second,
        // only while something plays.
        if self.media.as_ref().is_some_and(|media| media.playing) {
            if self.media_tick.is_none() {
                self.media_tick = Some(cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor().timer(TICK).await;
                        let Ok(playing) = this.update(cx, |this, cx| {
                            if this.open {
                                cx.notify();
                            }
                            this.media.as_ref().is_some_and(|media| media.playing)
                        }) else {
                            break;
                        };
                        if !playing {
                            let _ = this.update(cx, |this, _| this.media_tick = None);
                            break;
                        }
                    }
                }));
            }
        } else {
            self.media_tick = None;
        }
        self.keep_responsive();
        cx.notify();
    }

    /// Lets go of a cover that is no longer shown: its decoded picture and
    /// its texture, which GPUI would otherwise keep for every track played.
    fn forget_cover(cover: Option<(Arc<[u8]>, Arc<gpui::Image>)>, cx: &mut App) {
        if let Some((_, image)) = cover {
            gpui::ImageSource::Image(image).evict(cx);
        }
    }

    /// Asks for the lyrics of what plays, once per track, and only while
    /// Lyrics is on: off, no title leaves this Mac for them.
    pub(super) fn want_lyrics(&mut self, cx: &mut Context<Self>) {
        if !self.look.lyrics {
            return;
        }
        if let Some(track) = self.media.as_ref().map(|media| media.track.clone())
            && self.lyrics_for.as_ref() != Some(&track)
        {
            self.fetch_lyrics(track, cx);
        }
    }

    fn fetch_lyrics(&mut self, track: Track, cx: &mut Context<Self>) {
        self.lyrics_for = Some(track.clone());
        self.lyrics = None;
        self.lyrics_task = Some(cx.spawn(async move |this, cx| {
            let asked = track.clone();
            let lyrics = cx
                .background_executor()
                .spawn(async move { media_model::fetch(&asked) })
                .await
                // Offline or refused: no lyrics this time, nothing retried.
                .unwrap_or(Lyrics::Missing);
            let _ = this.update(cx, |this, cx| {
                if tracing() {
                    eprintln!(
                        "NOTCHTRACE lyrics {} {}",
                        track.label(),
                        match &lyrics {
                            Lyrics::Synced(lines) => format!("synced, {} lines", lines.len()),
                            Lyrics::Plain(lines) => format!("plain, {} lines", lines.len()),
                            other => format!("{other:?}"),
                        }
                    );
                }
                if this.lyrics_for.as_ref() == Some(&track) {
                    this.lyrics = Some((track, lyrics));
                    cx.notify();
                }
            });
        }));
    }

    /// Sends `command` to the player and shows its effect at once; the next
    /// read confirms it.
    pub(super) fn send_media(&mut self, command: platform::MediaCommand, cx: &mut Context<Self>) {
        if let Some(commands) = &self.media_commands {
            let _ = commands.send(command);
        }
        let now = Instant::now();
        if let Some(media) = self.media.as_mut() {
            match command {
                platform::MediaCommand::TogglePlay => {
                    media.position = media.position_at(now);
                    media.at = now;
                    media.playing = !media.playing;
                }
                platform::MediaCommand::Seek(to) => {
                    media.position = to;
                    media.at = now;
                }
                _ => {}
            }
        }
        cx.notify();
    }

    /// Whether the closed notch shows what plays: only while it plays.
    pub(super) fn media_showing(&self) -> bool {
        self.media
            .as_ref()
            .is_some_and(|media| media.playing && media.track.is_known())
    }

    /// Whether Home leads with the player: while there is a track, playing
    /// or paused, and the shelf is not being arranged.
    pub(super) fn player_shown(&self) -> bool {
        !self.editing
            && self
                .media
                .as_ref()
                .is_some_and(|media| media.track.is_known())
    }

    /// The lyrics for the track that plays now, once fetched.
    fn current_lyrics(&self) -> Option<&Lyrics> {
        let media = self.media.as_ref()?;
        self.lyrics
            .as_ref()
            .filter(|(track, _)| *track == media.track)
            .map(|(_, lyrics)| lyrics)
    }

    /// The player's switch for the lyrics on the closed notch: lit while
    /// they play there; only a word where the track has no timed lyrics.
    fn lyrics_toggle(
        &self,
        label: &'static str,
        timed: bool,
        accent: gpui::Hsla,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let on = self.look.lyrics && timed;
        div()
            .id("notch-media-lyrics")
            .debug_selector(|| "notch-media-lyrics".into())
            .role(Role::Switch)
            .aria_label("Lyrics on the closed notch")
            .flex_shrink_0()
            .h(px(18.))
            .px(px(7.))
            .flex()
            .items_center()
            .rounded(px(2.))
            .text_size(px(10.5))
            .when(on, |pill| pill.bg(accent).text_color(black(0.9)))
            .when(!on, |pill| pill.bg(ink(0.1)).text_color(ink(0.55)))
            .when(timed, |pill| {
                pill.cursor_pointer()
                    .hover(|style| style.opacity(0.85))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.change_look(|look| look.lyrics = !look.lyrics, cx)
                    }))
            })
            .child(label)
            .into_any_element()
    }

    /// For the closed notch: the line before, the line at the playhead and
    /// how long ago the song reached it; none where the lyrics are off, not
    /// timed or not in yet. Between verses it shows a note.
    pub(super) fn lyric_moment(&self) -> Option<(Option<String>, String, f32)> {
        if !self.look.lyrics {
            return None;
        }
        let media = self.media.as_ref()?;
        let Lyrics::Synced(lines) = self.current_lyrics()? else {
            return None;
        };
        // Each line starts in a little early, so it has risen into place
        // as it is sung rather than after.
        let position = media.position_at(Instant::now()) + LYRIC_LEAD;
        let Some(at) = media_model::line_at(lines, position) else {
            return Some((None, "♪".into(), 1.));
        };
        let text = |index: usize| {
            let text = lines[index].text.trim();
            if text.is_empty() {
                "♪".to_owned()
            } else {
                text.to_owned()
            }
        };
        let since = position.saturating_sub(lines[at].at).as_secs_f32();
        Some((at.checked_sub(1).map(text), text(at), since))
    }

    /// How much of the line at the playhead has been sung, 0 to 1: the
    /// line fills with the accent as far as this.
    fn lyric_sung(&self) -> f32 {
        let Some(media) = self.media.as_ref() else {
            return 0.;
        };
        let Some(Lyrics::Synced(lines)) = self.current_lyrics() else {
            return 0.;
        };
        // The same line the notch shows (it comes in a little early), filled
        // by where the song really is.
        let position = media.position_at(Instant::now());
        match media_model::line_at(lines, position + LYRIC_LEAD) {
            Some(at) => media_model::sung(lines, at, position),
            None => 0.,
        }
    }

    /// The cover on its way from the wing to the open player, drawn over
    /// the island while it springs open: it moves and grows with the
    /// island's own spring. `island` is the island now and `rest` the open
    /// panel it will rest at. None once it has landed.
    pub(super) fn flying_cover(&self, island: Frame, rest: Frame, cx: &App) -> Option<AnyElement> {
        let from = self.cover_flight?;
        if !self.player_shown() {
            return None;
        }
        let camera = self.camera(cx);
        let span = rest.height - from.height;
        if span <= 1. {
            return None;
        }
        // The spring overshoots a little, and so does the cover.
        let along = ((island.height - from.height) / span).clamp(0., 1.2);
        let lerp = |start: f32, end: f32| start + (end - start) * along;
        const WING_COVER: f32 = 20.;
        let side = lerp(WING_COVER, PLAYER_HEIGHT);
        let left = lerp(
            if camera.is_some() { 12. } else { 6. },
            (island.width - rest.width) / 2. + model::PANEL_PAD,
        );
        let top = lerp(
            (from.height - WING_COVER) / 2.,
            camera.map_or(0., |camera| camera.height) + model::PANEL_HEADER,
        );
        Some(
            div()
                .debug_selector(|| "notch-cover-flight".into())
                .absolute()
                .left(px(left))
                .top(px(top))
                .child(self.cover(side, false))
                .into_any_element(),
        )
    }

    /// While an agent works, a little of what plays beside it: the cover
    /// and the equalizer at the wing's end.
    pub(super) fn music_badge(&self) -> Option<AnyElement> {
        self.media_showing().then(|| {
            div()
                .debug_selector(|| "notch-music-badge".into())
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(5.))
                .child(self.cover(16., false))
                .child(equalizer(true))
                .into_any_element()
        })
    }

    fn cover(&self, side: f32, badge: bool) -> AnyElement {
        let source = self
            .media
            .as_ref()
            .map_or(Source::Other, |media| Source::of(&media.track));
        div()
            .relative()
            .size(px(side))
            .flex_shrink_0()
            .overflow_hidden()
            .rounded(px(if side > 40. { 4. } else { 2. }))
            .bg(ink(0.1))
            .child(match &self.art {
                Some((_, image)) => gpui::img(image.clone())
                    .size(px(side))
                    .object_fit(gpui::ObjectFit::Cover)
                    .into_any_element(),
                None => div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("media-note", side * 0.36).text_color(ink(0.45)))
                    .into_any_element(),
            })
            .when(badge, |cover| {
                cover.child(
                    div()
                        .absolute()
                        .right(px(4.))
                        .bottom(px(4.))
                        .child(source_mark(source, 20.)),
                )
            })
            .into_any_element()
    }

    /// What plays in one line: the lyric being sung, or the title gliding
    /// past.
    fn media_line(&self) -> AnyElement {
        let Some(media) = self.media.as_ref() else {
            return div().into_any_element();
        };
        match self.lyric_moment() {
            Some((previous, current, since)) => {
                let sung = self.lyric_sung();
                lyric_flow(previous, current, since, sung, media.playing).into_any_element()
            }
            None => glide(
                "notch-media-glide",
                media.track.label(),
                11.5,
                gpui::FontWeight::MEDIUM,
                ink(0.9),
            )
            .into_any_element(),
        }
    }

    /// The right wing while agents work under the music: the cover, the
    /// lyric or the title, and the equalizer.
    pub(super) fn music_wing(&self, width: f32) -> AnyElement {
        div()
            .debug_selector(|| "notch-music-wing".into())
            .w(px(width))
            .flex_shrink_0()
            .min_w_0()
            .pl(px(10.))
            .pr(px(14.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(self.cover(16., false))
            .child(self.media_line())
            .child(equalizer(true))
            .into_any_element()
    }

    /// The closed notch while something plays: the cover, the title gliding
    /// past and the equalizer.
    pub(super) fn media_wings(
        &self,
        camera: Option<model::Camera>,
        wings: (f32, f32),
    ) -> AnyElement {
        if self.media.is_none() {
            return div().into_any_element();
        }
        let title = self.media_line();
        match camera {
            Some(camera) => div()
                .debug_selector(|| "notch-media".into())
                .size_full()
                .flex()
                .items_center()
                .child(
                    div()
                        .w(px(wings.0))
                        .flex_shrink_0()
                        .pl(px(12.))
                        .pr(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(self.cover(20., false))
                        .child(title),
                )
                .child(div().w(px(camera.width)).flex_shrink_0())
                .child(
                    div()
                        .w(px(wings.1))
                        .flex_shrink_0()
                        .pr(px(14.))
                        .flex()
                        .justify_end()
                        .child(equalizer(true)),
                )
                .into_any_element(),
            None => div()
                .debug_selector(|| "notch-media".into())
                .size_full()
                .pl(px(6.))
                .pr(px(12.))
                .flex()
                .items_center()
                .gap(px(9.))
                .child(self.cover(20., false))
                .child(title)
                .child(equalizer(true))
                .into_any_element(),
        }
    }

    /// The player at the top of Home.
    pub(super) fn player_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(media) = self.media.clone() else {
            return div().into_any_element();
        };
        let now = Instant::now();
        let source = Source::of(&media.track);
        // The lyrics play on the closed notch; here only whether they do.
        let (lyrics_label, timed) = match self.current_lyrics() {
            // Off, nothing was asked for: the switch turns them on.
            _ if !self.look.lyrics => ("Lyrics", true),
            None => ("Lyrics…", false),
            Some(Lyrics::Synced(_)) => ("Lyrics", true),
            Some(Lyrics::Instrumental) => ("Instrumental", false),
            Some(Lyrics::Plain(_) | Lyrics::Missing) => ("No timed lyrics", false),
        };
        let scrubbing = self.scrub.get();
        let position = match (scrubbing, media.track.duration) {
            (Some(share), Some(duration)) => duration.mul_f32(share),
            _ => media.position_at(now),
        };
        let progress = scrubbing.or_else(|| media.progress_at(now)).unwrap_or(0.);
        let accent = match source {
            Source::Spotify => gpui::rgb(0x1db954).into(),
            _ => accent_color(),
        };
        let control = |id: &'static str, label: &'static str, glyph: &'static str, big: bool| {
            let side = if big { 30. } else { 26. };
            div()
                .id(id)
                .group(id)
                .debug_selector(move || id.into())
                .role(Role::Button)
                .aria_label(label)
                .size(px(side))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .rounded(px(2.))
                .when(big, |button| button.bg(ink(0.12)))
                .hover(|style| style.bg(ink(0.18)))
                .active(|style| style.bg(ink(0.24)))
                .child(
                    icon(glyph, if big { 15. } else { 12. })
                        .text_color(ink(0.92))
                        .group_hover(id, |style| style.text_color(ink(1.))),
                )
        };
        div()
            .debug_selector(|| "notch-player".into())
            .h(px(PLAYER_HEIGHT))
            .flex()
            .gap(px(12.))
            // While the cover flies in from the wing, its place waits empty.
            .child(
                div()
                    .flex_shrink_0()
                    .when(self.cover_flight.is_some(), |cover| cover.opacity(0.))
                    .child(self.cover(PLAYER_HEIGHT, true)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    // In a row of its own: the glide grows sideways, never
                    // into the column's height.
                    .child(div().flex_shrink_0().flex().child(glide(
                        "notch-player-title",
                        media.track.title.clone(),
                        14.,
                        gpui::FontWeight::SEMIBOLD,
                        ink(0.96),
                    )))
                    .child(
                        div()
                            .mt(px(1.))
                            .flex_shrink_0()
                            .h(px(17.))
                            .line_height(px(17.))
                            .truncate()
                            .text_size(px(12.))
                            .text_color(accent)
                            .child(if media.track.artist.trim().is_empty() {
                                media.track.album.clone()
                            } else {
                                media.track.artist.clone()
                            }),
                    )
                    .child(
                        div()
                            .mt(px(3.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .line_height(px(16.))
                                    .truncate()
                                    .text_size(px(11.))
                                    .text_color(ink(0.4))
                                    .child(media.track.album.clone()),
                            )
                            .child(self.lyrics_toggle(lyrics_label, timed, accent, cx)),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(time_label(position))
                            .child(self.scrubber(progress, media.track.duration, accent, cx))
                            .child(time_label(media.track.duration.unwrap_or_default())),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(14.))
                            .child(
                                control(
                                    "notch-media-previous",
                                    "Previous",
                                    "media-previous",
                                    false,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.send_media(platform::MediaCommand::Previous, cx)
                                    },
                                )),
                            )
                            .child(
                                control(
                                    "notch-media-play",
                                    if media.playing { "Pause" } else { "Play" },
                                    if media.playing {
                                        "media-pause"
                                    } else {
                                        "media-play"
                                    },
                                    true,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.send_media(platform::MediaCommand::TogglePlay, cx)
                                    },
                                )),
                            )
                            .child(
                                control("notch-media-next", "Next", "media-next", false).on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.send_media(platform::MediaCommand::Next, cx)
                                    }),
                                ),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// The bar through the track. Press anywhere on it and drag: the
    /// playhead follows the pointer and moves when the button comes up.
    fn scrubber(
        &self,
        progress: f32,
        duration: Option<Duration>,
        accent: gpui::Hsla,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let notch = cx.entity().downgrade();
        let scrub = self.scrub.clone();
        let dragging = scrub.get().is_some();
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let line = Bounds::new(
                    point(bounds.left(), bounds.center().y - px(1.5)),
                    gpui::size(bounds.size.width, px(3.)),
                );
                window.paint_quad(gpui::fill(line, ink(0.18)));
                window.paint_quad(gpui::fill(
                    Bounds::new(
                        line.origin,
                        gpui::size(bounds.size.width * progress, px(3.)),
                    ),
                    accent,
                ));
                let knob = px(if dragging { 11. } else { 9. });
                window.paint_quad(
                    gpui::fill(
                        Bounds::new(
                            point(
                                bounds.left() + bounds.size.width * progress - knob / 2.,
                                bounds.center().y - knob / 2.,
                            ),
                            gpui::size(knob, knob),
                        ),
                        ink(1.),
                    )
                    .corner_radii(px(2.)),
                );
                let Some(duration) = duration else {
                    return;
                };
                let share =
                    move |x: Pixels| ((x - bounds.left()) / bounds.size.width).clamp(0., 1.);
                {
                    let notch = notch.clone();
                    let scrub = scrub.clone();
                    window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
                        if phase.bubble()
                            && event.button == MouseButton::Left
                            && bounds.dilate(px(6.)).contains(&event.position)
                        {
                            scrub.set(Some(share(event.position.x)));
                            redraw(&notch, cx);
                            cx.stop_propagation();
                        }
                    });
                }
                {
                    let notch = notch.clone();
                    let scrub = scrub.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                        if phase.bubble() && scrub.get().is_some() && event.dragging() {
                            scrub.set(Some(share(event.position.x)));
                            redraw(&notch, cx);
                        }
                    });
                }
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if !phase.bubble() || event.button != MouseButton::Left {
                        return;
                    }
                    let Some(share) = scrub.take() else {
                        return;
                    };
                    let _ = notch.update(cx, |notch: &mut Notch, cx| {
                        notch.send_media(platform::MediaCommand::Seek(duration.mul_f32(share)), cx)
                    });
                });
            },
        )
        .flex_1()
        .h(px(14.))
        .into_any_element()
    }
}

fn redraw(notch: &WeakEntity<Notch>, cx: &mut App) {
    let _ = notch.update(cx, |_, cx| cx.notify());
}

fn time_label(at: Duration) -> impl IntoElement {
    let seconds = at.as_secs();
    div()
        .flex_shrink_0()
        .w(px(30.))
        .text_size(px(10.))
        .text_color(ink(0.5))
        .child(format!("{}:{:02}", seconds / 60, seconds % 60))
}

/// The app's mark on the cover: Spotify's own, a globe for a browser, a
/// note for anything else. Drawn, so no logo file ships.
fn source_mark(source: Source, side: f32) -> AnyElement {
    match source {
        Source::Spotify => canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let center = bounds.center();
                let r = bounds.size.width / 2.;
                window.paint_quad(gpui::fill(bounds, gpui::rgb(0x1db954)).corner_radii(r));
                // Three arcs, widest on top, as Spotify draws them.
                for (dy, half, width) in
                    [(-0.26, 0.58, 0.12), (0.02, 0.48, 0.1), (0.27, 0.38, 0.08)]
                {
                    let mut arc = gpui::PathBuilder::stroke(r * width * 2.);
                    let points: Vec<_> = (0..=8)
                        .map(|step| {
                            let t = -1. + 2. * step as f32 / 8.;
                            point(
                                center.x + r * half * t,
                                center.y + r * dy - r * 0.14 * (1. - t * t) + r * 0.07,
                            )
                        })
                        .collect();
                    arc.add_polygon(&points, false);
                    if let Ok(path) = arc.build() {
                        window.paint_path(path, gpui::black());
                    }
                }
            },
        )
        .size(px(side))
        .into_any_element(),
        other => div()
            .size(px(side))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(2.))
            .bg(black(0.7))
            .child(
                icon(
                    if other == Source::Browser {
                        "media-globe"
                    } else {
                        "media-note"
                    },
                    side * 0.55,
                )
                .text_color(ink(0.95)),
            )
            .into_any_element(),
    }
}

/// The waveform's bars: how wide each is and the gap between two.
const WAVE_BAR: f32 = 2.;
const WAVE_GAP: f32 = 1.5;
/// The room the waveform takes.
pub(super) const WAVE_WIDTH: f32 =
    media_model::WAVE as f32 * WAVE_BAR + (media_model::WAVE - 1) as f32 * WAVE_GAP;

/// What sounds, as a waveform in the accent: the last half second of the
/// output's real loudness, the newest on the right, each bar growing both
/// ways from the middle. Where the system has no meter the bars sway by
/// themselves; paused or under reduced motion they rest as a row of dots.
fn equalizer(playing: bool) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, cx| {
            let moving = playing && !cx.reduce_motion();
            let t = since_start();
            let middle = bounds.center().y;
            let heard = if moving { platform::audio_wave() } else { None };
            let accent = accent_color();
            for index in 0..media_model::WAVE {
                let place = index as f32;
                let level: f32 = match heard {
                    Some(wave) => wave[index],
                    None if moving => {
                        let (speed, phase) = (6.2 + (place * 1.7) % 3.1, place * 1.3);
                        0.25 + 0.75
                            * ((t * speed + phase).sin() * 0.5 + 0.5)
                            * ((t * speed * 0.37 + phase * 2.).sin() * 0.35 + 0.65)
                    }
                    None => 0.,
                };
                // Never less than a dot, so silence still shows the trace.
                let height = (bounds.size.height * level).max(px(WAVE_BAR));
                let x = bounds.left() + px(place * (WAVE_BAR + WAVE_GAP));
                // What sounded a moment ago fades as it travels left.
                let mut color = accent;
                color.a *= 0.45 + 0.55 * (place + 1.) / media_model::WAVE as f32;
                window.paint_quad(
                    gpui::fill(
                        Bounds::new(
                            point(x, middle - height / 2.),
                            gpui::size(px(WAVE_BAR), height),
                        ),
                        color,
                    )
                    .corner_radii(px(WAVE_BAR / 2.)),
                );
            }
            if moving {
                // On the shared clock, at the pace of what runs for a whole
                // song, and the frame keeps its measurements instead of
                // laying the notch out again.
                crate::pulse::request(crate::pulse::Rate::Steady, window, cx);
            }
        },
    )
    .w(px(WAVE_WIDTH))
    .h(px(14.))
    .flex_shrink_0()
}

/// The karaoke fill's soft edge, in points, and the slices it fades over.
const EDGE: f32 = 10.;
const SLICES: usize = 5;

/// One lyric line shaped in `color`.
fn lyric_line(
    text: &str,
    size: f32,
    weight: gpui::FontWeight,
    color: gpui::Hsla,
    window: &mut Window,
) -> gpui::ShapedLine {
    let mut font = gpui::font(crate::fonts::UI);
    font.weight = weight;
    let run = gpui::TextRun {
        len: text.len(),
        font,
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window.text_system().shape_line(
        gpui::SharedString::from(text.to_owned()),
        px(size),
        &[run],
        None,
    )
}

/// Over a line already drawn at `origin`, its sung part in the accent: whole
/// up to `front` points along it less the edge, then fading over a few
/// slices. Nothing is drawn outside `clip`.
#[allow(clippy::too_many_arguments)]
fn paint_sung(
    text: &str,
    size: f32,
    weight: gpui::FontWeight,
    origin: gpui::Point<Pixels>,
    front: f32,
    alpha: f32,
    clip: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    if front <= 0. || alpha <= 0. {
        return;
    }
    let accent = accent_color();
    let line_height = px((size * 1.35).round());
    let slice = EDGE / SLICES as f32;
    let parts = std::iter::once((0., (front - EDGE).max(0.), 1.)).chain((0..SLICES).map(|index| {
        let from = front - EDGE + index as f32 * slice;
        let fade = 1. - (index as f32 + 0.5) / SLICES as f32;
        (from.max(0.), (from + slice).max(0.), fade)
    }));
    window.with_content_mask(Some(gpui::ContentMask { bounds: clip }), |window| {
        for (from, to, fade) in parts {
            if to <= from {
                continue;
            }
            let mask = gpui::ContentMask {
                bounds: Bounds::new(
                    point(origin.x + px(from), clip.top()),
                    gpui::size(px(to - from), clip.size.height),
                ),
            };
            let line = lyric_line(text, size, weight, accent.opacity(alpha * fade), window);
            window.with_content_mask(Some(mask), |window| {
                let _ = line.paint(origin, line_height, gpui::TextAlign::Left, None, window, cx);
            });
        }
    });
}

/// How long a new lyric line takes to come in.
const LYRIC_IN: f32 = 0.32;
/// How early a line starts to come in: about its rise, so it rests on the
/// beat it is sung.
const LYRIC_LEAD: Duration = Duration::from_millis(300);

/// The lyric line at the playhead: it rises in from below with a short
/// motion blur (fading copies trailing it, gone as it settles) while the
/// line before rises out; a line too long to fit then glides to show the
/// rest. As it is sung the line fills with the accent from its start, like
/// a karaoke, up to `sung` (0 to 1) with a soft edge, and a long line moves
/// to keep that edge in view. It all follows from how long ago the song
/// reached the line, so a seek or a pause draws the right moment.
fn lyric_flow(
    previous: Option<String>,
    current: String,
    since: f32,
    sung: f32,
    playing: bool,
) -> impl IntoElement {
    const SIZE: f32 = 11.5;
    let line_height = (SIZE * 1.35).round();
    div()
        .debug_selector(|| "notch-lyric".into())
        .flex_1()
        .min_w_0()
        .h(px(22.))
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, cx| {
                    let still = cx.reduce_motion();
                    let t = if still {
                        1.
                    } else {
                        (since / LYRIC_IN).clamp(0., 1.)
                    };
                    // Ease out: quick as it starts, settling softly.
                    let eased = 1. - (1. - t).powi(3);
                    let rise = f32::from(bounds.size.height) * 0.75;
                    let top = bounds.top() + (bounds.size.height - px(line_height)) / 2.;
                    // Under reduced motion the line shows whole, as before.
                    let fills = !still && current != "♪";
                    let accent = accent_color();
                    let colored = |text: &str, color: gpui::Hsla, window: &mut Window| {
                        lyric_line(text, SIZE, gpui::FontWeight::MEDIUM, color, window)
                    };
                    let shape = |text: &str, alpha: f32, window: &mut Window| {
                        colored(text, ink(0.92 * alpha), window)
                    };
                    // A new line rises in at the clock's full pace; the fill
                    // then runs on at half for as long as the line is sung.
                    let rising = t < 1.;
                    let mut moving = rising;
                    window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                        if let Some(previous) = previous.as_deref().filter(|_| t < 1.) {
                            // The line before leaves as it was: sung.
                            let line = if fills && previous != "♪" {
                                colored(previous, accent.opacity(1. - eased), window)
                            } else {
                                shape(previous, 1. - eased, window)
                            };
                            let _ = line.paint(
                                point(bounds.left(), top - px(rise * eased)),
                                px(line_height),
                                gpui::TextAlign::Left,
                                None,
                                window,
                                cx,
                            );
                        }
                        // The blur: copies trailing the line as it moves,
                        // fainter the further back, gone once it rests.
                        let speed = 1. - eased;
                        if speed > 0.02 {
                            for trail in 1..=3 {
                                let line = shape(&current, eased * 0.28 / trail as f32, window);
                                let _ = line.paint(
                                    point(
                                        bounds.left(),
                                        top + px(rise * speed + trail as f32 * 3. * speed),
                                    ),
                                    px(line_height),
                                    gpui::TextAlign::Left,
                                    None,
                                    window,
                                    cx,
                                );
                            }
                        }
                        // What is still to sing waits dimmer than what was sung.
                        let line = shape(&current, eased * if fills { 0.6 } else { 1. }, window);
                        let width = f32::from(line.width);
                        let overflow = width - f32::from(bounds.size.width);
                        // Where the fill has reached along the line.
                        let front = (width + EDGE) * sung.clamp(0., 1.);
                        let offset = if overflow <= 0. || still {
                            0.
                        } else if fills {
                            // Too long: it moves to keep the fill's edge in view.
                            (front - f32::from(bounds.size.width) * 0.55).clamp(0., overflow)
                        } else if t >= 1. {
                            // Settled and too long: glide through it once.
                            moving = true;
                            ((since - LYRIC_IN - 0.6).max(0.) * GLIDE_SPEED).min(overflow)
                        } else {
                            0.
                        };
                        let origin =
                            point(bounds.left() - px(offset), top + px(rise * (1. - eased)));
                        let _ = line.paint(
                            origin,
                            px(line_height),
                            gpui::TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                        if fills {
                            paint_sung(
                                &current,
                                SIZE,
                                gpui::FontWeight::MEDIUM,
                                origin,
                                front,
                                eased,
                                bounds,
                                window,
                                cx,
                            );
                        }
                        // The fill runs on while the song does.
                        moving |= fills && playing && sung < 1.;
                    });
                    if moving {
                        // On the shared clock, and the frame keeps its
                        // measurements instead of laying the notch out again.
                        let rate = if rising {
                            crate::pulse::Rate::Smooth
                        } else {
                            crate::pulse::Rate::Steady
                        };
                        crate::pulse::request(rate, window, cx);
                    }
                },
            )
            .size_full(),
        )
}

/// One line of text that fits, or glides left to show the rest: it waits a
/// moment at its start, slides through and comes round again.
fn glide(
    id: &'static str,
    text: String,
    size: f32,
    weight: gpui::FontWeight,
    color: gpui::Hsla,
) -> impl IntoElement {
    let text: gpui::SharedString = text.into();
    let line_height = (size * 1.35).round();
    div()
        .id(id)
        .debug_selector(move || id.into())
        .flex_1()
        .min_w_0()
        .h(px(line_height))
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, cx| {
                    let mut font = gpui::font(crate::fonts::UI);
                    font.weight = weight;
                    let run = gpui::TextRun {
                        len: text.len(),
                        font,
                        color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    let shaped =
                        window
                            .text_system()
                            .shape_line(text.clone(), px(size), &[run], None);
                    let width = shaped.width;
                    let top = bounds.top() + (bounds.size.height - px(line_height)) / 2.;
                    if width <= bounds.size.width {
                        let _ = shaped.paint(
                            point(bounds.left(), top),
                            px(line_height),
                            gpui::TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                        return;
                    }
                    let lap = f32::from(width) + GLIDE_GAP;
                    let still = cx.reduce_motion();
                    let offset = if still {
                        0.
                    } else {
                        let phase = since_start() % (GLIDE_PAUSE + lap / GLIDE_SPEED);
                        ((phase - GLIDE_PAUSE) * GLIDE_SPEED).max(0.)
                    };
                    window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                        for start in [0., lap] {
                            let _ = shaped.paint(
                                point(bounds.left() + px(start - offset), top),
                                px(line_height),
                                gpui::TextAlign::Left,
                                None,
                                window,
                                cx,
                            );
                        }
                    });
                    if !still {
                        // On the shared clock, at the pace of what runs for
                        // a whole song, and the frame keeps its measurements
                        // instead of laying the notch out again.
                        crate::pulse::request(crate::pulse::Rate::Steady, window, cx);
                    }
                },
            )
            .size_full(),
        )
}

/// A week at a glance: the month, and the days of this week with today in
/// the accent; one column shows just today.
pub(super) fn calendar_widget(span: u32) -> AnyElement {
    use chrono::Datelike;
    let today = chrono::Local::now().date_naive();
    if span < 2 {
        return div()
            .flex_1()
            .flex()
            .flex_col()
            .justify_end()
            .child(
                div()
                    .text_size(px(24.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(accent_color())
                    .child(today.day().to_string()),
            )
            .child(
                div()
                    .text_size(px(10.5))
                    .text_color(ink(0.5))
                    .child(today.format("%a · %b %Y").to_string()),
            )
            .into_any_element();
    }
    let monday = today - chrono::Days::new(u64::from(today.weekday().num_days_from_monday()));
    let mut days = div().flex_1().flex().items_end().justify_between();
    for offset in 0..7 {
        let day = monday + chrono::Days::new(offset);
        let is_today = day == today;
        days = days.child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(3.))
                .child(
                    div()
                        .text_size(px(9.5))
                        .text_color(ink(if is_today { 0.85 } else { 0.4 }))
                        .child(day.format("%a").to_string()),
                )
                .child(
                    div()
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(2.))
                        .text_size(px(12.))
                        .font_weight(if is_today {
                            gpui::FontWeight::SEMIBOLD
                        } else {
                            gpui::FontWeight::NORMAL
                        })
                        .when(is_today, |cell| {
                            cell.bg(accent_color()).text_color(white(1.))
                        })
                        .when(!is_today, |cell| cell.text_color(ink(0.75)))
                        .child(day.day().to_string()),
                ),
        );
    }
    div()
        .flex_1()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(
            div()
                .text_size(px(10.5))
                .text_color(ink(0.5))
                .child(today.format("%B %Y").to_string()),
        )
        .child(days)
        .into_any_element()
}

/// The battery's color as every system paints it: green while it charges,
/// red when it runs low. The same in any theme, like the notch's black.
pub(super) fn charge_color(charge: f32, charging: bool) -> gpui::Hsla {
    if charging {
        gpui::rgb(0x30d158).into()
    } else if charge <= 0.2 {
        gpui::rgb(0xff453a).into()
    } else {
        ink(0.85)
    }
}

/// The battery in the header: its charge, a bolt while it charges; nothing
/// on a computer without one.
pub(super) fn battery_chip() -> Option<AnyElement> {
    let (charge, charging) = platform::battery()?;
    let fill = charge_color(charge, charging);
    Some(
        div()
            .debug_selector(|| "notch-battery".into())
            .flex()
            .items_center()
            .gap(px(5.))
            .text_size(px(11.))
            .text_color(ink(0.7))
            .child(format!("{}%", (charge * 100.).round() as u32))
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .w(px(20.))
                            .h(px(10.))
                            .p(px(1.5))
                            .border_1()
                            .border_color(ink(0.45))
                            .rounded(px(2.))
                            .child(div().h_full().w(gpui::relative(charge)).bg(fill)),
                    )
                    .child(div().w(px(1.5)).h(px(4.)).bg(ink(0.45))),
            )
            .when(charging, |chip| {
                chip.child(icon("media-charging", 10.).text_color(charge_color(charge, true)))
            })
            .into_any_element(),
    )
}
