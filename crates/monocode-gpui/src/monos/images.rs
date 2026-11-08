//! A Mono's pictures (`images.generate`, ImageGeneration.tsx): the chat gets
//! a frame per take whose cells breathe under a "Creating" pill while Gemini
//! makes them through a throwaway Antigravity turn, then the pictures take
//! the frames' place. The turn and the files are core's (monocode_core::images).
use super::*;
use crate::image_lightbox;
use gpui::{Animation, AnimationExt, CursorStyle, ElementId, ObjectFit, img};
use monocode_core::images::{self, GeneratedImage, ImageJob, ImageRequest, JobStatus};
use monocode_core::mono::{bot_tools, bridge::Call};
use monocode_core::time::now_ms;
use serde_json::Value;
use std::path::Path;

/// A job still making pictures this long after it started was cut off by a quit.
const STALE_MS: u64 = 10 * 60_000;
/// Cells a side in a frame's breathing grid.
const CELLS: usize = 6;

/// The card a job is in the chat: the job itself, typed "images".
fn job_card(job: &ImageJob) -> Value {
    let mut card = serde_json::to_value(job).unwrap_or_default();
    card["type"] = "images".into();
    card
}

/// Makes the pictures, blocking: one Antigravity turn, the pictures copied
/// into the app's `generated-images` folder.
fn make(request: &ImageRequest) -> Result<Vec<GeneratedImage>, String> {
    if let Some(source) = &request.source
        && !Path::new(source).is_file()
    {
        return Err(format!("source image not found: {source}"));
    }
    let data = monocode_core::skills::bundled::data_dir().ok_or("No data folder for pictures")?;
    let runner = images::AntigravityRunner::installed(data.join(images::GENERATED_IMAGE_DIR))?;
    images::run_image_turn(request, &runner)
}

impl Monos {
    /// `images.generate`: a Mono's own chat, and a bot only with Images in its
    /// tools. The frames go in the chat and the call answers at once; the
    /// pictures replace the frames when Gemini is done.
    pub(super) fn images_generate(
        &mut self,
        call: &Call,
        mono: &Mono,
        input: &Value,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        if mono.session_id != Some(call.session) {
            return Err(images::ONLY_MONO_ERROR.into());
        }
        if mono.is_bot() {
            let role = mono.bot_role.as_deref().unwrap_or("assistant");
            if !bot_tools::tools(role, mono.bot_tools.as_ref())
                .access
                .iter()
                .any(|access| access == "images")
            {
                return Err(images::IMAGES_OFF_ERROR.into());
            }
        }
        let request = images::parse_request(input)?;
        let job = ImageJob::start(images::new_job_id(), &request);
        self.hold_card(call.session, &mono.id, job_card(&job));
        self.save(cx);
        let response = images::action_response(&job.id, &request);
        let work = cx.background_executor().spawn({
            let request = request.clone();
            async move { make(&request) }
        });
        let id = mono.id.clone();
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                let job = match result {
                    Ok(images) => job.done(images),
                    Err(error) => job.failed(error),
                };
                this.update_image_card(&id, job_card(&job), cx);
            });
        })
        .detach();
        Ok(response)
    }

    /// Puts a job's new state where its card is: in the chat, or still
    /// waiting to be posted after the turn.
    fn update_image_card(&mut self, mono: &str, card: Value, cx: &mut Context<Self>) {
        let id = card["id"].as_str().unwrap_or_default().to_owned();
        let same = |text: &str| {
            serde_json::from_str::<Value>(text).is_ok_and(|value| value["id"] == id.as_str())
        };
        if let Some(waiting) = self.roster.get_mut(mono).and_then(|m| {
            m.pending_reports
                .iter_mut()
                .find(|report| report.strip_prefix("mono-card:").is_some_and(same))
        }) {
            *waiting = format!("mono-card:{card}");
            self.save(cx);
            return;
        }
        if let Some(held) = self
            .running
            .values_mut()
            .flat_map(|run| run.cards.iter_mut())
            .find(|held| held["id"] == id.as_str())
        {
            *held = card;
            return;
        }
        let Some(chat) = self.chats.get(mono).cloned() else {
            return;
        };
        let index = chat
            .read(cx)
            .mono_rows(cx)
            .iter()
            .rposition(|row| row.role == "mono-card" && same(&row.text));
        if let Some(index) = index {
            let _ = chat.update(cx, |chat, cx| chat.mono_patch_card(index, &id, card, cx));
        }
    }
}

/// The frames' width over height for a "16:9"-style aspect (frameRatio).
fn frame_ratio(aspect: Option<&str>) -> f32 {
    images::styles::ratio(aspect)
}

/// CSS ease-in-out, near enough for a breathing cell.
fn ease_in_out(t: f32) -> f32 {
    t * t * (3. - 2. * t)
}

/// The job card (ImageGeneration).
pub(super) fn render(
    value: &Value,
    cx: &mut Context<crate::transcript::TranscriptView>,
) -> AnyElement {
    let Ok(job) = serde_json::from_value::<ImageJob>(value.clone()) else {
        return div().into_any_element();
    };
    let age = now_ms().saturating_sub(job.started_at);
    let interrupted = job.status == JobStatus::Generating && age > STALE_MS;
    let making = job.status == JobStatus::Generating && !interrupted;
    let done = job.status == JobStatus::Done;
    let ratio = frame_ratio(job.aspect.as_deref());
    let frames = if done {
        job.images.len()
    } else {
        job.count.clamp(1, 4)
    };
    let columns: u16 = if frames == 1 { 1 } else { 2 };
    let max_w = if ratio != 1. && columns == 1 {
        520.
    } else if columns == 1 {
        320.
    } else {
        460.
    };
    let reduce = cx.reduce_motion();
    let tiles = (0..frames)
        .map(|index| match job.images.get(index).filter(|_| done) {
            Some(image) => picture(&job, index, image, ratio),
            None => frame(&job, index, ratio, making, reduce, age),
        })
        .collect::<Vec<_>>();
    let failed = interrupted || job.status == JobStatus::Failed;
    let note = if interrupted {
        "Stopped when MonoBOT closed. Ask again to retry.".to_owned()
    } else if failed {
        job.error
            .clone()
            .unwrap_or_else(|| "Could not make the pictures.".into())
    } else {
        job.prompt.clone()
    };
    div()
        .id(ElementId::Name(format!("image-job-{}", job.id).into()))
        .debug_selector(|| "mono-image-job".into())
        .min_w_0()
        .pt(px(8.))
        .pb(px(12.))
        .child(
            div()
                .grid()
                .grid_cols(columns)
                .gap(px(8.))
                .w_full()
                .max_w(px(max_w))
                .children(tiles),
        )
        .child(
            div()
                .mt(px(6.))
                .flex()
                .min_w_0()
                .items_center()
                .gap(px(6.))
                .text_size(px(11.))
                .text_color(content(0.45))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(content(0.6))
                        .child("Nano Banana"),
                )
                .child("·")
                .when_some(
                    job.style.as_deref().and_then(images::styles::style),
                    |d, style| {
                        d.child(
                            div()
                                .flex_shrink_0()
                                .text_color(content(0.6))
                                .child(style.name),
                        )
                        .child("·")
                    },
                )
                .child(
                    div()
                        .id("image-job-note")
                        .min_w_0()
                        .truncate()
                        .when(failed, |d| d.role(Role::Alert).text_color(content(0.6)))
                        .child(note),
                ),
        )
        .into_any_element()
}

/// A take still being made: a grid of cells breathing along the diagonal
/// under a "Creating" pill (`.image-cell`, 1.8s, 0.35 → 1 → 0.35).
fn frame(
    job: &ImageJob,
    index: usize,
    ratio: f32,
    making: bool,
    reduce: bool,
    age: u64,
) -> AnyElement {
    let cells = (0..CELLS * CELLS).map(|cell| {
        let base = div().rounded(px(3.)).bg(content(0.06));
        if !making {
            return base.opacity(0.4).into_any_element();
        }
        if reduce {
            return base.opacity(0.6).into_any_element();
        }
        let (row, column) = (cell / CELLS, cell % CELLS);
        let delay = ((row + column) * 90 + index * 240) as f32 / 1800.;
        base.with_animation(
            ElementId::NamedInteger(format!("image-cell-{}-{index}", job.id).into(), cell as u64),
            Animation::new(Duration::from_millis(1800)).repeat(),
            move |cell, delta| {
                let phase = (delta - delay).rem_euclid(1.);
                let rise = if phase < 0.5 {
                    phase * 2.
                } else {
                    (1. - phase) * 2.
                };
                cell.opacity(0.35 + 0.65 * ease_in_out(rise))
            },
        )
        .into_any_element()
    });
    div()
        .id(ElementId::NamedInteger(
            format!("image-frame-{}", job.id).into(),
            index as u64,
        ))
        .relative()
        .overflow_hidden()
        .rounded_xl()
        .border_1()
        .border_color(content(0.1))
        .bg(content(0.03))
        .w_full()
        .aspect_ratio(ratio)
        .when(making, |d| {
            d.role(Role::Status).aria_label("Making a picture")
        })
        .child(
            div()
                .absolute()
                .inset_0()
                .grid()
                .grid_cols(CELLS as u16)
                .grid_rows(CELLS as u16)
                .gap(px(1.))
                .p(px(1.))
                .children(cells),
        )
        .when(making, |d| {
            d.child(
                div()
                    .absolute()
                    .bottom(px(8.))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .rounded_full()
                            .border_1()
                            .border_color(content(0.1))
                            .bg(background_base().opacity(0.8))
                            .px(px(10.))
                            .py(px(2.))
                            .text_size(px(11.))
                            .text_color(content(0.7))
                            .child(crate::activity::shimmer(
                                "Creating".into(),
                                age as f32 / 1000.,
                            )),
                    ),
            )
        })
        .into_any_element()
}

/// A finished take: fades in where its frame was and opens full screen.
fn picture(job: &ImageJob, index: usize, image: &GeneratedImage, ratio: f32) -> AnyElement {
    let path = PathBuf::from(&image.path);
    let open = image_lightbox::Open {
        source: image_lightbox::Source::Path(path.clone()),
        alt: job.prompt.clone().into(),
    };
    let name = image.name.clone();
    let missing = name.clone();
    div()
        .id(ElementId::NamedInteger(
            format!("image-picture-{}", job.id).into(),
            index as u64,
        ))
        .debug_selector(move || format!("mono-image-{index}"))
        .relative()
        .overflow_hidden()
        .rounded_xl()
        .border_1()
        .border_color(content(0.1))
        .bg(content(0.03))
        .w_full()
        .aspect_ratio(ratio)
        .role(Role::Button)
        .aria_label(format!("Open {name} full screen"))
        .cursor(CursorStyle::PointingHand)
        .on_click(move |_, window, cx| {
            window.dispatch_action(Box::new(open.clone()), cx);
        })
        .child(
            img(path)
                .size_full()
                .object_fit(ObjectFit::Cover)
                .with_fallback(move || {
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .p(px(12.))
                        .text_size(px(11.))
                        .text_color(content(0.5))
                        .child(format!("Could not open {missing}"))
                        .into_any_element()
                })
                .with_animation(
                    ElementId::Name(format!("image-shown-{}", image.path).into()),
                    Animation::new(Duration::from_millis(500)).with_easing(gpui::ease_out_quint()),
                    |picture, delta| picture.opacity(delta),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_take_the_job_aspect() {
        assert_eq!(frame_ratio(None), 1.);
        assert_eq!(frame_ratio(Some("16:9")), 16. / 9.);
        assert_eq!(frame_ratio(Some("3:4")), 0.75);
        assert_eq!(frame_ratio(Some("wide")), 1.);
        assert_eq!(frame_ratio(Some("0:4")), 1.);
    }

    #[test]
    fn a_job_card_is_the_job_typed_images() {
        let request =
            images::parse_request(&serde_json::json!({"prompt": "a fox", "count": 2})).unwrap();
        let job = ImageJob::start("job-1", &request);
        let card = job_card(&job);
        assert_eq!(card["type"], "images");
        assert_eq!(card["id"], "job-1");
        assert_eq!(card["count"], 2);
        let back: ImageJob = serde_json::from_value(card).unwrap();
        assert_eq!(back, job);
    }
}
