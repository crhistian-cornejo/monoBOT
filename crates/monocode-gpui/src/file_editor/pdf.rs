//! A read-only PDF preview. Windows renders pages with its PDF API, off the
//! UI thread; no document JavaScript or embedded HTML is executed.
use super::*;

pub(super) struct PdfEvent(pub String);

pub(super) struct PdfView {
    path: String,
    bytes: Arc<[u8]>,
    page: usize,
    pages: usize,
    image: Option<Arc<RenderImage>>,
    error: Option<String>,
    generation: u64,
    _render: Option<Task<()>>,
    text_input: Entity<Input>,
    text: Option<monocode_core::papers::PaperText>,
    text_error: Option<String>,
    text_mode: bool,
    pending_focus: Option<(usize, Option<String>)>,
    _text_task: Option<Task<()>>,
    _text_events: Subscription,
}

impl EventEmitter<PdfEvent> for PdfView {}

impl PdfView {
    pub(super) fn new(path: String, bytes: Vec<u8>, cx: &mut Context<Self>) -> Self {
        cx.on_release(|this, cx| {
            if let Some(image) = this.image.take() {
                cx.drop_image(image, None);
            }
        })
        .detach();
        let text_input = cx.new(|cx| {
            let mut input = Input::editor(cx);
            input.set_read_only(true);
            input
        });
        let text_events = cx.observe(&text_input, |_, _, cx| cx.notify());
        let mut view = Self {
            path,
            bytes: bytes.into(),
            page: 0,
            pages: 0,
            image: None,
            error: None,
            generation: 0,
            _render: None,
            text_input,
            text: None,
            text_error: None,
            text_mode: false,
            pending_focus: None,
            _text_task: None,
            _text_events: text_events,
        };
        view.render_page(cx);
        view.load_text(cx);
        view
    }

    fn load_text(&mut self, cx: &mut Context<Self>) {
        let bytes = self.bytes.clone();
        let work = cx
            .background_executor()
            .spawn(async move { monocode_core::papers::extract_pdf_text(&bytes) });
        self._text_task = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(text) => {
                        this.text = Some(text);
                        this.update_text(cx);
                        this.apply_focus(cx);
                    }
                    Err(error) => {
                        this.text_error = Some(error);
                        this.apply_focus(cx);
                    }
                }
                cx.notify();
            });
        }));
    }

    fn update_text(&self, cx: &mut Context<Self>) {
        let text = self
            .text
            .as_ref()
            .and_then(|text| text.pages.get(self.page))
            .filter(|text| !text.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| "No readable text is available for this page.".into());
        self.text_input
            .update(cx, |input, cx| input.set_text(text, cx));
    }

    fn selected_quote(&self, cx: &App) -> Option<String> {
        let input = self.text_input.read(cx);
        let range = input.selection_range();
        let page = self.text.as_ref()?.pages.get(self.page)?;
        if page.trim().is_empty() || range.is_empty() {
            return None;
        }
        let selected = input.content.get(range)?;
        let clean = selected.split_whitespace().collect::<Vec<_>>().join(" ");
        if clean.is_empty() {
            return None;
        }
        let truncated = clean.chars().count() > 8_000;
        let mut clean: String = clean.chars().take(8_000).collect();
        if truncated {
            clean.push_str(" [selection truncated]");
        }
        Some(format!(
            "{clean}\n\n- {} (page {})",
            self.path,
            self.page + 1
        ))
    }

    pub(super) fn focus_passage(
        &mut self,
        page: usize,
        passage: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.pending_focus = Some((page.max(1), passage));
        self.apply_focus(cx);
    }

    fn apply_focus(&mut self, cx: &mut Context<Self>) {
        let Some((page, passage)) = self.pending_focus.clone() else {
            return;
        };
        if self.pages == 0 {
            return;
        }
        let page = page.min(self.pages).saturating_sub(1);
        if page != self.page {
            self.page = page;
            self.update_text(cx);
            self.render_page(cx);
        }
        let Some(passage) = passage else {
            self.pending_focus = None;
            return;
        };
        if self.text.is_none() && self.text_error.is_none() {
            return;
        }
        if let Some(text) = self.text.as_ref().and_then(|text| text.pages.get(page)) {
            if let Some(found) = monocode_core::papers::locate_in_items(&[text], &passage) {
                let quote = passage.trim();
                let selection = text
                    .find(quote)
                    .map(|start| start..start + quote.len())
                    .unwrap_or(found.start_offset..found.end_offset);
                self.text_mode = true;
                self.text_input.update(cx, |input, cx| {
                    input.select_range(selection, cx);
                });
            }
        }
        self.pending_focus = None;
        cx.notify();
    }

    pub(super) fn has_bytes(&self, bytes: &[u8]) -> bool {
        self.bytes.as_ref() == bytes
    }

    fn render_page(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        if let Some(old) = self.image.take() {
            cx.drop_image(old, None);
        }
        self.error = None;
        let generation = self.generation;
        let page = self.page;
        let bytes = self.bytes.clone();
        let renderer = cx.svg_renderer();
        let work = cx.background_executor().spawn(async move {
            let (count, png) = render(&bytes, page)?;
            let image = Image::from_bytes(ImageFormat::Png, png)
                .to_image_data(renderer)
                .map_err(|error| format!("Could not decode the PDF page: {error}"))?;
            Ok::<_, String>((count, image))
        });
        self._render = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    if let Ok((_, image)) = result {
                        cx.drop_image(image, None);
                    }
                    return;
                }
                match result {
                    Ok((count, image)) => {
                        if let Some(old) = this.image.replace(image) {
                            cx.drop_image(old, None);
                        }
                        this.pages = count;
                        this.error = None;
                        this.apply_focus(cx);
                    }
                    Err(error) => {
                        if let Some(old) = this.image.take() {
                            cx.drop_image(old, None);
                        }
                        this.error = Some(error);
                    }
                }
                cx.notify();
            });
        }));
    }

    fn navigate(&mut self, next: bool, cx: &mut Context<Self>) {
        let page = if next {
            (self.page + 1).min(self.pages.saturating_sub(1))
        } else {
            self.page.saturating_sub(1)
        };
        if page != self.page {
            self.page = page;
            self.update_text(cx);
            self.render_page(cx);
            cx.notify();
        }
    }
}

impl Render for PdfView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(32.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .border_b_1()
                    .border_color(content(0.1))
                    .text_size(px(12.))
                    .text_color(content(0.6))
                    .child(
                        div()
                            .id("pdf-previous")
                            .role(Role::Button)
                            .aria_label("Previous page")
                            .px_2()
                            .rounded_md()
                            .hover(|s| s.bg(content(0.1)))
                            .child("Previous")
                            .on_click(cx.listener(|this, _, _, cx| this.navigate(false, cx))),
                    )
                    .child(if self.pages == 0 {
                        "PDF".to_owned()
                    } else {
                        format!("Page {} of {}", self.page + 1, self.pages)
                    })
                    .child(
                        div()
                            .id("pdf-next")
                            .role(Role::Button)
                            .aria_label("Next page")
                            .px_2()
                            .rounded_md()
                            .hover(|s| s.bg(content(0.1)))
                            .child("Next")
                            .on_click(cx.listener(|this, _, _, cx| this.navigate(true, cx))),
                    )
                    .child(
                        div()
                            .id("pdf-toggle-text")
                            .role(Role::Button)
                            .aria_label(if self.text_mode { "Show page" } else { "Show text" })
                            .px_2()
                            .rounded_md()
                            .hover(|s| s.bg(content(0.1)))
                            .child(if self.text_mode { "Show page" } else { "Show text" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.text_mode = !this.text_mode;
                                cx.notify();
                            })),
                    )
                    .when(self.text_mode && self.selected_quote(cx).is_some(), |d| {
                        d.child(
                            div()
                                .id("pdf-add-to-chat")
                                .role(Role::Button)
                                .aria_label("Add selection to chat")
                                .px_2()
                                .rounded_md()
                                .hover(|s| s.bg(content(0.1)))
                                .child("Add to chat")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(quote) = this.selected_quote(cx) {
                                        cx.emit(PdfEvent(quote));
                                    }
                                })),
                        )
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p_3()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(self.text_mode && self.text.is_some(), |d| {
                        d.child(self.text_input.clone())
                    })
                    .when(self.text_mode && self.text.is_none(), |d| {
                        d.text_size(px(12.))
                            .text_color(content(0.5))
                            .child(self.text_error.clone().unwrap_or_else(|| "Reading PDF text...".into()))
                    })
                    .when(!self.text_mode && self.image.is_none() && self.error.is_none(), |d| {
                        d.text_size(px(12.))
                            .text_color(content(0.5))
                            .child("Opening PDF page...")
                    })
                    .when_some((!self.text_mode).then(|| self.image.clone()).flatten(), |d, image| {
                        d.child(img(ImageSource::Render(image)).size_full().object_fit(ObjectFit::Contain))
                    })
                    .when_some((!self.text_mode).then(|| self.error.clone()).flatten(), |d, error| {
                        d.flex_col()
                            .gap_2()
                            .text_size(px(12.))
                            .text_color(content(0.6))
                            .child(error)
                            .when(!self.path.starts_with("remote://"), |d| d.child(
                                div()
                                    .id("pdf-open-externally")
                                    .role(Role::Button)
                                    .aria_label("Open PDF externally")
                                    .px_3()
                                    .py_1()
                                    .rounded_md()
                                    .bg(content(0.1))
                                    .child("Open externally")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let path = PathBuf::from(&this.path);
                                        cx.background_executor().spawn(async move {
                                            let _ = monocode_core::files::open_path_with_default_app(&path);
                                        }).detach();
                                    })),
                            ))
                    }),
            )
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn render(_: &[u8], _: usize) -> Result<(usize, Vec<u8>), String> {
    Err("PDF preview is not available on this platform yet.".into())
}

#[cfg(target_os = "windows")]
pub(crate) fn render(bytes: &[u8], page_index: usize) -> Result<(usize, Vec<u8>), String> {
    use windows::{
        Data::Pdf::{PdfDocument, PdfPageRenderOptions},
        Storage::Streams::{DataReader, DataWriter, InMemoryRandomAccessStream},
        Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    };

    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { RoUninitialize() };
        }
    }
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|error| error.to_string())?;
    let _apartment = Apartment;
    let draw = || -> windows::core::Result<(usize, Vec<u8>)> {
        let input = InMemoryRandomAccessStream::new()?;
        let writer = DataWriter::CreateDataWriter(&input)?;
        writer.WriteBytes(bytes)?;
        writer.StoreAsync()?.join()?;
        writer.DetachStream()?;
        input.Seek(0)?;
        let document = PdfDocument::LoadFromStreamAsync(&input)?.join()?;
        let pages = document.PageCount()? as usize;
        if page_index >= pages {
            return Err(windows::core::Error::from(
                windows::Win32::Foundation::E_INVALIDARG,
            ));
        }
        let page = document.GetPage(page_index as u32)?;
        let size = page.Size()?;
        let scale = (1400.0 / size.Width.max(1.0)).min(4096.0 / size.Height.max(1.0));
        let options = PdfPageRenderOptions::new()?;
        options.SetDestinationWidth((size.Width * scale).round().max(1.0) as u32)?;
        options.SetDestinationHeight((size.Height * scale).round().max(1.0) as u32)?;
        let output = InMemoryRandomAccessStream::new()?;
        page.RenderWithOptionsToStreamAsync(&output, &options)?
            .join()?;
        page.Close()?;
        let length = u32::try_from(output.Size()?)
            .map_err(|_| windows::core::Error::from(windows::Win32::Foundation::E_INVALIDARG))?;
        let reader = DataReader::CreateDataReader(&output.GetInputStreamAt(0)?)?;
        reader.LoadAsync(length)?.join()?;
        let mut png = vec![0; length as usize];
        reader.ReadBytes(&mut png)?;
        Ok((pages, png))
    };
    draw().map_err(|error| format!("Could not open this PDF: {error}"))
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn two_page_pdf() -> Vec<u8> {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] /Resources << >> /Contents 5 0 R >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] /Resources << >> /Contents 5 0 R >>",
            "<< /Length 0 >>\nstream\n\nendstream",
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref = pdf.len();
        pdf.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    #[test]
    fn windows_renders_pdf_pages_and_reports_invalid_documents() {
        std::thread::spawn(|| {
            let pdf = two_page_pdf();
            for page in 0..2 {
                let (count, png) = render(&pdf, page).unwrap();
                assert_eq!(count, 2);
                assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
            }
            assert!(render(&pdf, 2).is_err());
            assert!(render(b"%PDF-1.4\ninvalid", 0).is_err());
        })
        .join()
        .unwrap();
    }

    #[gpui::test]
    fn citation_focus_waits_for_page_count_and_selects_quoted_text(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| PdfView::new("paper.pdf".into(), two_page_pdf(), cx))
            })
            .unwrap()
        });
        window
            .update(cx, |view, _, cx| {
                let quote = "The reported result preserves the complete data.";
                view.text = Some(monocode_core::papers::PaperText {
                    pages: vec!["The first page.".into(), format!("Before. {quote} After.")],
                    ..Default::default()
                });
                view.focus_passage(2, Some(quote.into()), cx);
                assert_eq!(view.page, 0);
                assert!(view.pending_focus.is_some());
                view.pages = 2;
                view.apply_focus(cx);
                assert_eq!(view.page, 1);
                assert!(view.text_mode);
                assert!(view.pending_focus.is_none());
                let input = view.text_input.read(cx);
                assert_eq!(&input.content[input.selection_range()], quote);
                assert!(view.selected_quote(cx).unwrap().contains("(page 2)"));
            })
            .unwrap();
    }
}
