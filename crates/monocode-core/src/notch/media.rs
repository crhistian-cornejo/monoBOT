//! What plays on the computer, for the notch: the track the system's media
//! controls report (Spotify, a browser's video, the media player) and its
//! synced lyrics from LRCLIB (`lrclib.net`, free and without a key).
//!
//! The platform reads the track; this module keeps the rules: where the
//! playhead is now, which lyric line it is on, and how to ask LRCLIB.
use std::time::{Duration, Instant};

/// A track as the system's media controls describe it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Track {
    /// The app that plays it, as the system names it (`Spotify.exe`, a
    /// browser's id…).
    pub app: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// The length, where the app says it.
    pub duration: Option<Duration>,
}

impl Track {
    /// Whether there is anything worth showing: a video without a title, or
    /// an app that only claims the controls, is not.
    pub fn is_known(&self) -> bool {
        !self.title.trim().is_empty()
    }

    /// One short line: "Title · Artist", or the title alone.
    pub fn label(&self) -> String {
        if self.artist.trim().is_empty() {
            self.title.trim().to_owned()
        } else {
            format!("{} · {}", self.title.trim(), self.artist.trim())
        }
    }
}

/// The track now, with the playhead as last reported.
#[derive(Clone, Debug, PartialEq)]
pub struct Playing {
    pub track: Track,
    pub playing: bool,
    /// The playhead when it was read.
    pub position: Duration,
    /// When it was read.
    pub at: Instant,
    /// The cover as the app hands it over (PNG or JPEG bytes), if any.
    pub art: Option<std::sync::Arc<[u8]>>,
}

impl Playing {
    /// The playhead at `now`: still while paused, running while playing,
    /// never past the end.
    pub fn position_at(&self, now: Instant) -> Duration {
        let position = if self.playing {
            self.position + now.saturating_duration_since(self.at)
        } else {
            self.position
        };
        match self.track.duration {
            Some(duration) if !duration.is_zero() => position.min(duration),
            _ => position,
        }
    }

    /// How far through the track, 0 to 1, where its length is known.
    pub fn progress_at(&self, now: Instant) -> Option<f32> {
        let duration = self.track.duration.filter(|d| !d.is_zero())?;
        Some((self.position_at(now).as_secs_f32() / duration.as_secs_f32()).clamp(0., 1.))
    }
}

/// The playhead between an app's reports. An app stamps its position when
/// it reports one, so a new position plus the time since its stamp is where
/// the song is. Spotify also restamps every few seconds without moving the
/// position; read the same way, that would pull the playhead back each time
/// and the lyrics with it. So while the reported position and the play state
/// stay the same, the playhead runs on its own clock from the last report.
#[derive(Clone, Copy, Debug, Default)]
pub struct Playhead {
    last: Option<(Duration, bool, Duration, Instant)>,
}

impl Playhead {
    /// Where the song is at `now`, given the app's `reported` position,
    /// whether it plays and how old the app's stamp is.
    pub fn read(
        &mut self,
        reported: Duration,
        playing: bool,
        stamp_age: Option<Duration>,
        now: Instant,
    ) -> Duration {
        match self.last {
            Some((raw, was_playing, anchor, at)) if raw == reported && was_playing == playing => {
                if playing {
                    anchor + now.saturating_duration_since(at)
                } else {
                    anchor
                }
            }
            _ => {
                let anchor = match stamp_age {
                    Some(age) if playing => reported + age,
                    _ => reported,
                };
                self.last = Some((reported, playing, anchor, now));
                anchor
            }
        }
    }
}

/// One timed line of synced lyrics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub at: Duration,
    pub text: String,
}

/// What LRCLIB knows about a track.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lyrics {
    /// Lines with their times, in order.
    Synced(Vec<Line>),
    /// Words without times.
    Plain(Vec<String>),
    /// A track without words.
    Instrumental,
    /// LRCLIB has nothing for it.
    Missing,
}

impl Lyrics {
    /// The line to show at `position`, and the one after it.
    pub fn lines_at(&self, position: Duration) -> (Option<&str>, Option<&str>) {
        let Lyrics::Synced(lines) = self else {
            return (None, None);
        };
        match line_at(lines, position) {
            Some(index) => (
                Some(lines[index].text.as_str()),
                lines.get(index + 1).map(|line| line.text.as_str()),
            ),
            None => (None, lines.first().map(|line| line.text.as_str())),
        }
    }
}

/// The lines of an LRC file: every `[mm:ss.xx]` tag on a line gives that
/// line a time; tags such as `[ar:…]` and lines without a time are skipped.
/// Empty lines are kept, as the pauses between verses.
pub fn parse_lrc(text: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    for raw in text.lines() {
        let mut rest = raw.trim();
        let mut times = Vec::new();
        while let Some(tag) = rest.strip_prefix('[') {
            let Some(end) = tag.find(']') else {
                break;
            };
            let Some(at) = parse_time(&tag[..end]) else {
                break;
            };
            times.push(at);
            rest = tag[end + 1..].trim_start();
        }
        for at in times {
            lines.push(Line {
                at,
                text: rest.trim().to_owned(),
            });
        }
    }
    lines.sort_by_key(|line| line.at);
    lines
}

/// `mm:ss`, `mm:ss.xx` or `mm:ss.xxx`.
fn parse_time(tag: &str) -> Option<Duration> {
    let (minutes, seconds) = tag.split_once(':')?;
    let minutes: u64 = minutes.trim().parse().ok()?;
    let (whole, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    let whole: u64 = whole.trim().parse().ok()?;
    if whole >= 60 || !fraction.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let millis = match fraction.len() {
        0 => 0,
        1 => fraction.parse::<u64>().ok()? * 100,
        2 => fraction.parse::<u64>().ok()? * 10,
        _ => fraction[..3].parse::<u64>().ok()?,
    };
    Some(Duration::from_millis(
        (minutes * 60 + whole) * 1000 + millis,
    ))
}

/// The last line that has started by `position`.
pub fn line_at(lines: &[Line], position: Duration) -> Option<usize> {
    lines
        .partition_point(|line| line.at <= position)
        .checked_sub(1)
}

/// How much of line `index` has been sung at `position`, 0 to 1. LRCLIB
/// times lines, not words, so the line fills evenly over the time it is
/// sung: until the next line starts, or, before a pause or at the end, for
/// as long as its words would take.
pub fn sung(lines: &[Line], index: usize, position: Duration) -> f32 {
    let Some(line) = lines.get(index) else {
        return 0.;
    };
    let letters = line.text.chars().filter(|c| !c.is_whitespace()).count();
    if letters == 0 {
        return 0.;
    }
    let spoken = (0.6 + letters as f32 * 0.11).clamp(1.2, 9.);
    let length = match lines.get(index + 1) {
        Some(next) => next.at.saturating_sub(line.at).as_secs_f32().min(spoken),
        None => spoken,
    };
    if length <= 0. {
        return 1.;
    }
    (position.saturating_sub(line.at).as_secs_f32() / length).clamp(0., 1.)
}

/// How many bars the waveform draws.
pub const WAVE: usize = 9;

/// The waveform out of the output's loudness: the last moments of what
/// sounded, oldest first, scrolling as it plays. The system gives one
/// number, the peak of what sounds now, about thirty times a second; every
/// other reading starts a new bar and the one between raises it, so a bar
/// is the loudest of some 66 ms and the whole trace a little over half a
/// second: a beat is seen entering on the right and travelling left.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wave {
    levels: [f32; WAVE],
    /// Whether the newest bar is still being filled.
    filling: bool,
}

impl Wave {
    /// Takes the peak now (0 to 1) and gives the bars, each 0 to 1.
    pub fn feed(&mut self, peak: f32) -> [f32; WAVE] {
        // Quiet passages still move the bars: loudness is not heard evenly.
        let level = if peak.is_finite() {
            peak.clamp(0., 1.).sqrt()
        } else {
            0.
        };
        if self.filling {
            self.levels[WAVE - 1] = self.levels[WAVE - 1].max(level);
        } else {
            self.levels.rotate_left(1);
            self.levels[WAVE - 1] = level;
        }
        self.filling = !self.filling;
        self.levels
    }
}

const LRCLIB: &str = "https://lrclib.net/api";

/// LRCLIB's exact lookup for `track`; it matches best with the album and the
/// length, which it uses to pick between versions.
pub fn lookup_url(track: &Track) -> Option<String> {
    if !track.is_known() || track.artist.trim().is_empty() {
        return None;
    }
    let mut url = url::Url::parse(&format!("{LRCLIB}/get")).ok()?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("artist_name", track.artist.trim());
        query.append_pair("track_name", track.title.trim());
        if !track.album.trim().is_empty() {
            query.append_pair("album_name", track.album.trim());
        }
        if let Some(duration) = track.duration.filter(|d| !d.is_zero()) {
            query.append_pair("duration", &duration.as_secs().to_string());
        }
    }
    Some(url.into())
}

/// LRCLIB's search, for when the exact lookup misses (a video's title, an
/// album LRCLIB names differently).
pub fn search_url(track: &Track) -> Option<String> {
    if !track.is_known() {
        return None;
    }
    let mut url = url::Url::parse(&format!("{LRCLIB}/search")).ok()?;
    url.query_pairs_mut()
        .append_pair("track_name", track.title.trim());
    if !track.artist.trim().is_empty() {
        url.query_pairs_mut()
            .append_pair("artist_name", track.artist.trim());
    }
    Some(url.into())
}

/// The lyrics in one LRCLIB record.
pub fn from_record(record: &serde_json::Value) -> Lyrics {
    if record["instrumental"].as_bool() == Some(true) {
        return Lyrics::Instrumental;
    }
    if let Some(synced) = record["syncedLyrics"].as_str() {
        let lines = parse_lrc(synced);
        if !lines.is_empty() {
            return Lyrics::Synced(lines);
        }
    }
    match record["plainLyrics"].as_str() {
        Some(plain) if !plain.trim().is_empty() => {
            Lyrics::Plain(plain.lines().map(str::to_owned).collect())
        }
        _ => Lyrics::Missing,
    }
}

/// The best of a search's records. Timed lyrics belong to one cut of a
/// song, so a record as long as the track (within 3 s) comes first: an
/// extended mix's lines run late against the radio edit. Then any timed
/// lyrics, then any words.
pub fn from_search(records: &serde_json::Value, duration: Option<Duration>) -> Lyrics {
    let Some(records) = records.as_array() else {
        return Lyrics::Missing;
    };
    let fits = |record: &serde_json::Value| match (duration, record["duration"].as_f64()) {
        (Some(duration), Some(length)) => (duration.as_secs_f64() - length).abs() <= 3.,
        _ => false,
    };
    let found: Vec<(bool, Lyrics)> = records
        .iter()
        .map(|record| (fits(record), from_record(record)))
        .collect();
    let synced = |(_, lyrics): &&(bool, Lyrics)| matches!(lyrics, Lyrics::Synced(_));
    found
        .iter()
        .find(|found| found.0 && synced(found))
        .or_else(|| found.iter().find(synced))
        .or_else(|| {
            found
                .iter()
                .find(|(_, lyrics)| !matches!(lyrics, Lyrics::Missing))
        })
        .map(|(_, lyrics)| lyrics.clone())
        .unwrap_or(Lyrics::Missing)
}

/// Asks LRCLIB for `track`'s lyrics: the exact lookup, then the search.
/// Blocking; run it off the interface thread, once per track.
pub fn fetch(track: &Track) -> Result<Lyrics, String> {
    let agent = monobot_http::agent()
        .timeout(Duration::from_secs(10))
        .user_agent(concat!(
            "MonoBOT/",
            env!("CARGO_PKG_VERSION"),
            " (https://github.com/crhistian-cornejo/monoBOT)"
        ))
        .build();
    let get = |url: &str| -> Result<Option<serde_json::Value>, String> {
        match agent.get(url).call() {
            Ok(response) => {
                let body = response.into_string().map_err(|error| error.to_string())?;
                serde_json::from_str(&body)
                    .map(Some)
                    .map_err(|error| error.to_string())
            }
            Err(ureq::Error::Status(404, _)) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    };
    if let Some(url) = lookup_url(track) {
        if let Some(record) = get(&url)? {
            let lyrics = from_record(&record);
            if !matches!(lyrics, Lyrics::Missing) {
                return Ok(lyrics);
            }
        }
    }
    match search_url(track) {
        Some(url) => Ok(get(&url)?.map_or(Lyrics::Missing, |records| {
            from_search(&records, track.duration)
        })),
        None => Ok(Lyrics::Missing),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> Track {
        Track {
            app: "Spotify.exe".into(),
            title: "Bohemian Rhapsody".into(),
            artist: "Queen".into(),
            album: "A Night at the Opera".into(),
            duration: Some(Duration::from_secs(354)),
        }
    }

    #[test]
    fn lrc_lines_take_every_time_tag_and_skip_metadata() {
        let lines = parse_lrc(
            "[ar:Queen]\n[ti:Song]\n[00:01.50]Is this the real life?\n\
             [00:05.2][01:10.250] Is this just fantasy?\n[00:09.00]\nno time here\n",
        );
        let at: Vec<_> = lines.iter().map(|l| l.at.as_millis()).collect();
        assert_eq!(at, vec![1500, 5200, 9000, 70250]);
        assert_eq!(lines[1].text, "Is this just fantasy?");
        assert_eq!(lines[2].text, "");
        assert_eq!(lines[3].text, "Is this just fantasy?");
    }

    #[test]
    fn the_line_follows_the_playhead() {
        let lyrics = Lyrics::Synced(parse_lrc("[00:01.00]one\n[00:03.00]two\n[00:05.00]three"));
        assert_eq!(
            lyrics.lines_at(Duration::from_millis(500)),
            (None, Some("one"))
        );
        assert_eq!(
            lyrics.lines_at(Duration::from_millis(3000)),
            (Some("two"), Some("three"))
        );
        assert_eq!(
            lyrics.lines_at(Duration::from_secs(60)),
            (Some("three"), None)
        );
        assert_eq!(
            Lyrics::Plain(vec!["words".into()]).lines_at(Duration::ZERO),
            (None, None)
        );
    }

    #[test]
    fn the_waveform_scrolls_what_sounded_with_the_newest_last() {
        let mut wave = Wave::default();
        // A beat enters on the right as its own bar.
        let first = wave.feed(0.25);
        assert_eq!(first[WAVE - 1], 0.5);
        assert!(first[..WAVE - 1].iter().all(|bar| *bar == 0.));
        // The reading between two bars only raises the newest.
        assert_eq!(wave.feed(0.04)[WAVE - 1], 0.5);
        assert_eq!(wave.feed(1.)[WAVE - 2], 0.5);
        let now = wave.feed(0.);
        assert_eq!((now[WAVE - 2], now[WAVE - 1]), (0.5, 1.));
        // It travels left and leaves after the trace's length.
        for _ in 0..(2 * WAVE) {
            wave.feed(0.);
        }
        assert_eq!(wave.feed(0.), [0.; WAVE]);
        // Odd readings are silence, never a spike.
        assert_eq!(wave.feed(f32::NAN)[WAVE - 1], 0.);
        assert_eq!(wave.feed(7.)[WAVE - 1], 1.);
    }

    #[test]
    fn a_line_fills_as_it_is_sung() {
        let lines = parse_lrc(
            "[00:10.00]Is this the real life?\n[00:12.00]Is this just fantasy?\n\
             [00:14.00]\n[00:40.00]Caught in a landslide",
        );
        let at = |millis| Duration::from_millis(millis);
        // Before the line, at its half and past its end.
        assert_eq!(sung(&lines, 0, at(9_000)), 0.);
        assert!((sung(&lines, 0, at(11_000)) - 0.5).abs() < 1e-4);
        assert_eq!(sung(&lines, 0, at(12_500)), 1.);
        // A pause between verses has no words to fill.
        assert_eq!(sung(&lines, 2, at(20_000)), 0.);
        // The last line, and one before a long pause, take as long as their
        // words would, not until whatever comes next.
        let last = sung(&lines, 3, at(41_000));
        assert!(last > 0.2 && last < 0.6, "{last}");
        assert_eq!(sung(&lines, 3, at(50_000)), 1.);
        assert_eq!(sung(&lines, 9, at(50_000)), 0.);
    }

    #[test]
    fn the_playhead_runs_while_playing_and_stops_at_the_end() {
        let at = Instant::now();
        let mut playing = Playing {
            track: track(),
            playing: true,
            position: Duration::from_secs(10),
            at,
            art: None,
        };
        assert_eq!(
            playing.position_at(at + Duration::from_secs(5)),
            Duration::from_secs(15)
        );
        assert_eq!(
            playing.position_at(at + Duration::from_secs(1000)),
            Duration::from_secs(354)
        );
        playing.playing = false;
        assert_eq!(
            playing.position_at(at + Duration::from_secs(5)),
            Duration::from_secs(10)
        );
        assert!((playing.progress_at(at).unwrap() - 10. / 354.).abs() < 1e-4);
    }

    #[test]
    fn a_restamp_without_a_new_position_never_pulls_the_playhead_back() {
        let start = Instant::now();
        let at = |seconds: f32| start + Duration::from_secs_f32(seconds);
        let mut playhead = Playhead::default();
        // Reported 10 s, stamped 0.2 s ago: the song is at 10.2 s.
        let first = playhead.read(
            Duration::from_secs(10),
            true,
            Some(Duration::from_millis(200)),
            at(0.),
        );
        assert_eq!(first, Duration::from_millis(10_200));
        // Spotify restamps 4.5 s later without moving the position: the
        // playhead has run on, not gone back to 10 s.
        let later = playhead.read(Duration::from_secs(10), true, Some(Duration::ZERO), at(4.5));
        assert_eq!(later, Duration::from_millis(14_700));
        // A new position is taken as the truth again.
        let fresh = playhead.read(Duration::from_secs(15), true, Some(Duration::ZERO), at(5.));
        assert_eq!(fresh, Duration::from_secs(15));
        // Paused, it holds where the app says.
        let paused = playhead.read(
            Duration::from_secs(15),
            false,
            Some(Duration::from_secs(3)),
            at(8.),
        );
        assert_eq!(paused, Duration::from_secs(15));
        assert_eq!(
            playhead.read(Duration::from_secs(15), false, None, at(20.)),
            Duration::from_secs(15)
        );
    }

    #[test]
    fn lrclib_urls_carry_the_track_encoded() {
        let url = lookup_url(&track()).unwrap();
        assert_eq!(
            url,
            "https://lrclib.net/api/get?artist_name=Queen&track_name=Bohemian+Rhapsody\
             &album_name=A+Night+at+the+Opera&duration=354"
        );
        let mut video = track();
        video.artist.clear();
        assert_eq!(lookup_url(&video), None);
        assert_eq!(
            search_url(&video).unwrap(),
            "https://lrclib.net/api/search?track_name=Bohemian+Rhapsody"
        );
        video.title = "  ".into();
        assert_eq!(search_url(&video), None);
    }

    #[test]
    fn records_prefer_synced_lyrics() {
        let record = serde_json::json!({
            "instrumental": false,
            "plainLyrics": "one\ntwo",
            "syncedLyrics": "[00:01.00]one\n[00:02.00]two",
        });
        assert!(matches!(from_record(&record), Lyrics::Synced(lines) if lines.len() == 2));
        let plain = serde_json::json!({ "plainLyrics": "one\ntwo", "syncedLyrics": null });
        assert_eq!(
            from_record(&plain),
            Lyrics::Plain(vec!["one".into(), "two".into()])
        );
        assert_eq!(
            from_record(&serde_json::json!({ "instrumental": true })),
            Lyrics::Instrumental
        );
        let search = serde_json::json!([plain, record]);
        assert!(matches!(from_search(&search, None), Lyrics::Synced(_)));
        assert_eq!(from_search(&serde_json::json!([]), None), Lyrics::Missing);
        // The cut as long as the track wins over an earlier, longer one.
        let cuts = serde_json::json!([
            { "duration": 300.0, "syncedLyrics": "[00:20.00]extended" },
            { "duration": 197.0, "syncedLyrics": "[00:05.00]radio edit" },
        ]);
        assert!(matches!(
            from_search(&cuts, Some(Duration::from_secs(198))),
            Lyrics::Synced(lines) if lines[0].text == "radio edit"
        ));
    }
}
