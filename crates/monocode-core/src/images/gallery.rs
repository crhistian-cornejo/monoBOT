//! An image bot's gallery: every picture its chat holds, newest first, read
//! from the transcript so it needs no store of its own and forgets a picture
//! when its chat does. Pictures come from `images.generate` jobs (the chat's
//! `mono-card` rows typed "images") and from pictures a provider made during
//! an ordinary turn (`image` rows).
use super::{styles, GeneratedImage, ImageJob, JobStatus, REPLY_ROLE};
use crate::transcript::Row;
use serde_json::Value;

/// One tile of the gallery.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// Stable across renders: the job and take, or the picture's path.
    pub key: String,
    /// `None` while the picture is still being made.
    pub path: Option<String>,
    pub prompt: String,
    /// A preset style's id.
    pub style: Option<String>,
    /// "16:9" and the like; square when unset.
    pub aspect: Option<String>,
    /// Unix milliseconds, when known.
    pub created_at: Option<u64>,
}

impl Item {
    pub fn making(&self) -> bool {
        self.path.is_none()
    }
    /// A short name for the tile: the brief's first clause.
    pub fn title(&self) -> String {
        title(&self.prompt)
    }
    pub fn style_name(&self) -> Option<&'static str> {
        self.style
            .as_deref()
            .and_then(styles::style)
            .map(|s| s.name)
    }
    pub fn orientation(&self) -> styles::Orientation {
        styles::Orientation::of(self.aspect.as_deref())
    }
    /// "Portrait · 4:5", "Square · 1:1", or the bare ratio.
    pub fn format_label(&self) -> String {
        let aspect = self.aspect.as_deref().unwrap_or("1:1");
        match styles::format_for_aspect(aspect) {
            Some(format) => format!("{} · {aspect}", format.name),
            None => aspect.to_owned(),
        }
    }
}

/// "Today 10:42", "Yesterday 18:05", "Nov 3" or "Nov 3, 2025": when a
/// picture was made, as the gallery's hover card says it.
pub fn date_label(at_ms: u64, now: chrono::DateTime<chrono::Local>) -> Option<String> {
    use chrono::{Datelike, TimeZone};
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let at = chrono::Local
        .timestamp_millis_opt(i64::try_from(at_ms).ok()?)
        .single()?;
    let days = (now.date_naive() - at.date_naive()).num_days();
    let time = at.format("%H:%M");
    Some(match days {
        0 => format!("Today {time}"),
        1 => format!("Yesterday {time}"),
        _ if at.year() == now.year() => format!("{} {}", MONTHS[at.month0() as usize], at.day()),
        _ => format!(
            "{} {}, {}",
            MONTHS[at.month0() as usize],
            at.day(),
            at.year()
        ),
    })
}

/// The first clause of a brief, at most `MAX` characters, cut on a word.
pub fn title(prompt: &str) -> String {
    const MAX: usize = 56;
    let prompt = prompt.trim();
    let clause = prompt
        .split(['.', '\n', ';', ':'])
        .map(str::trim)
        .find(|part| !part.is_empty())
        .unwrap_or(prompt);
    if clause.chars().count() <= MAX {
        return clause.to_owned();
    }
    let cut: String = clause.chars().take(MAX).collect();
    let cut = cut
        .rsplit_once(' ')
        .map_or(cut.as_str(), |(head, _)| head)
        .trim_end_matches([',', ' ']);
    format!("{cut}…")
}

/// The gallery of a chat, newest first. Failed and empty jobs leave nothing.
pub fn from_rows(rows: &[Row]) -> Vec<Item> {
    let mut items = Vec::new();
    let mut last_sent = None;
    for row in rows {
        if let Some(sent) = row.sent_at {
            last_sent = Some(sent);
        }
        match row.role.as_str() {
            "mono-card" => {
                let Ok(value) = serde_json::from_str::<Value>(&row.text) else {
                    continue;
                };
                if value["type"] != "images" {
                    continue;
                }
                let Ok(job) = serde_json::from_value::<ImageJob>(value) else {
                    continue;
                };
                items.extend(job_items(&job));
            }
            role if role == REPLY_ROLE => {
                let Ok(image) = serde_json::from_str::<GeneratedImage>(&row.text) else {
                    continue;
                };
                items.push(Item {
                    key: image.path.clone(),
                    prompt: image.alt.clone().unwrap_or_else(|| image.name.clone()),
                    path: Some(image.path),
                    style: None,
                    aspect: None,
                    created_at: last_sent,
                });
            }
            _ => {}
        }
    }
    items.reverse();
    items
}

fn job_items(job: &ImageJob) -> Vec<Item> {
    let item = |index: usize, path: Option<String>| Item {
        key: format!("{}#{index}", job.id),
        path,
        prompt: job.prompt.clone(),
        style: job.style.clone(),
        aspect: job.aspect.clone(),
        created_at: Some(job.started_at),
    };
    match job.status {
        JobStatus::Done => job
            .images
            .iter()
            .enumerate()
            .map(|(index, image)| item(index, Some(image.path.clone())))
            .collect(),
        JobStatus::Generating => (0..job.count.clamp(1, super::MAX_TAKES))
            .map(|index| item(index, None))
            .collect(),
        JobStatus::Failed => Vec::new(),
    }
}

/// Which pictures the gallery shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Filter {
    #[default]
    All,
    Only(styles::Orientation),
}

impl Filter {
    pub fn keeps(self, item: &Item) -> bool {
        match self {
            Self::All => true,
            Self::Only(orientation) => item.orientation() == orientation,
        }
    }
}

/// Spreads tiles over `columns` columns, each to the shortest so far, the way
/// a masonry wall fills: returns the item indexes of each column in order.
pub fn masonry(items: &[&Item], columns: usize) -> Vec<Vec<usize>> {
    let columns = columns.max(1);
    let mut placed = vec![Vec::new(); columns];
    let mut heights = vec![0f32; columns];
    for (index, item) in items.iter().enumerate() {
        let shortest = (0..columns)
            .min_by(|a, b| heights[*a].total_cmp(&heights[*b]))
            .unwrap_or(0);
        placed[shortest].push(index);
        // Height of a unit-wide tile, plus the gap between tiles.
        heights[shortest] += 1. / styles::ratio(item.aspect.as_deref()) + 0.05;
    }
    placed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(role: &str, text: String, sent_at: Option<u64>) -> Row {
        Row {
            role: role.into(),
            text,
            first: true,
            sent_at,
        }
    }

    fn card(id: &str, status: &str, images: &[&str], extra: Value) -> Row {
        let mut card = json!({
            "type": "images",
            "id": id,
            "prompt": "Vintage editorial illustration of a fox. Blue background.",
            "count": 2,
            "status": status,
            "startedAt": 1_000,
            "images": images.iter().map(|path| json!({
                "path": path, "name": "x.png", "mimeType": "image/png", "size": 1
            })).collect::<Vec<_>>(),
        });
        for (key, value) in extra.as_object().unwrap() {
            card[key] = value.clone();
        }
        row("mono-card", card.to_string(), None)
    }

    #[test]
    fn reads_jobs_and_provider_pictures_newest_first() {
        let rows = vec![
            row("user", "make a fox".into(), Some(500)),
            card(
                "a",
                "done",
                &["/p/1.png", "/p/2.png"],
                json!({"style": "screenprint", "aspect": "4:5"}),
            ),
            card("b", "failed", &[], json!({"error": "quota"})),
            row("user", "another".into(), Some(2_000)),
            row(
                "image",
                json!({"path": "/p/3.png", "name": "3.png", "mimeType": "image/png", "size": 1, "alt": "A cat"}).to_string(),
                None,
            ),
            card("c", "generating", &[], json!({"aspect": "16:9"})),
            row("mono-card", json!({"type": "news"}).to_string(), None),
            row("mono-card", "not json".into(), None),
        ];
        let items = from_rows(&rows);
        let keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(keys, ["c#1", "c#0", "/p/3.png", "a#1", "a#0"]);
        assert!(items[0].making());
        assert_eq!(items[2].prompt, "A cat");
        assert_eq!(items[2].created_at, Some(2_000));
        let first = &items[4];
        assert_eq!(first.path.as_deref(), Some("/p/1.png"));
        assert_eq!(first.style_name(), Some("Screenprint poster"));
        assert_eq!(first.format_label(), "Portrait · 4:5");
        assert_eq!(first.title(), "Vintage editorial illustration of a fox");
        assert_eq!(items[2].format_label(), "Square · 1:1");
    }

    #[test]
    fn dates_read_as_today_yesterday_or_the_day() {
        use chrono::TimeZone;
        let now = chrono::Local
            .with_ymd_and_hms(2026, 11, 8, 10, 50, 0)
            .unwrap();
        let ms = |y, m, d, h, min| {
            chrono::Local
                .with_ymd_and_hms(y, m, d, h, min, 0)
                .unwrap()
                .timestamp_millis() as u64
        };
        assert_eq!(
            date_label(ms(2026, 11, 8, 9, 5), now).unwrap(),
            "Today 09:05"
        );
        assert_eq!(
            date_label(ms(2026, 11, 7, 18, 0), now).unwrap(),
            "Yesterday 18:00"
        );
        assert_eq!(date_label(ms(2026, 3, 2, 1, 0), now).unwrap(), "Mar 2");
        assert_eq!(
            date_label(ms(2025, 12, 31, 1, 0), now).unwrap(),
            "Dec 31, 2025"
        );
    }

    #[test]
    fn titles_are_short_and_cut_on_a_word() {
        assert_eq!(title("  a fox  "), "a fox");
        let long = "Vintage editorial sports illustration of Lionel Messi dynamically dribbling a classic ball";
        let short = title(long);
        assert!(
            short.ends_with('…') && short.chars().count() <= 57,
            "{short}"
        );
        assert!(long.starts_with(short.trim_end_matches('…')));
    }

    #[test]
    fn filters_by_orientation() {
        let item = |aspect: Option<&str>| Item {
            key: String::new(),
            path: None,
            prompt: String::new(),
            style: None,
            aspect: aspect.map(str::to_owned),
            created_at: None,
        };
        let portrait = Filter::Only(styles::Orientation::Portrait);
        assert!(portrait.keeps(&item(Some("9:16"))));
        assert!(!portrait.keeps(&item(None)));
        assert!(Filter::All.keeps(&item(Some("21:9"))));
    }

    #[test]
    fn masonry_fills_the_shortest_column() {
        let make = |aspect: &str| Item {
            key: String::new(),
            path: None,
            prompt: String::new(),
            style: None,
            aspect: Some(aspect.into()),
            created_at: None,
        };
        let tall = make("9:16");
        let wide = make("16:9");
        let items = [&tall, &wide, &wide, &wide, &wide];
        // The tall tile fills column 0; the wide ones stack in column 1
        // until it is taller.
        assert_eq!(masonry(&items, 2), [vec![0, 4], vec![1, 2, 3]]);
        assert_eq!(masonry(&items, 0), [vec![0, 1, 2, 3, 4]]);
    }
}
