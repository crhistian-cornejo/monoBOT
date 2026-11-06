use super::*;
use crate::office::fixtures::{package, png, rels};

const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;

fn deck() -> Vec<u8> {
    let presentation = format!(
        r#"<p:presentation {NS}><p:sldIdLst><p:sldId id="257" r:id="rId2"/><p:sldId id="256" r:id="rId1"/><p:sldId id="258" r:id="rId3"/></p:sldIdLst><p:sldSz cx="9144000" cy="5143500"/></p:presentation>"#
    );
    let presentation_rels = rels(&[
        ("rId1", "slide", "slides/slide1.xml"),
        ("rId2", "slide", "slides/slide2.xml"),
        ("rId3", "slide", "slides/slide3.xml"),
    ]);
    let theme = format!(
        r#"<a:theme {NS}><a:themeElements><a:clrScheme name="x"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1><a:accent1><a:srgbClr val="1F6FEB"/></a:accent1></a:clrScheme></a:themeElements></a:theme>"#
    );
    let master = format!(
        r#"<p:sldMaster {NS}><p:cSld><p:bg><p:bgPr><a:solidFill><a:srgbClr val="FAFAFA"/></a:solidFill></p:bgPr></p:bg><p:spTree/></p:cSld><p:txStyles><p:titleStyle><a:lvl1pPr algn="ctr"><a:defRPr sz="4000" b="1"/></a:lvl1pPr></p:titleStyle><p:bodyStyle><a:lvl1pPr><a:buChar char="•"/><a:defRPr sz="2000"/></a:lvl1pPr></p:bodyStyle></p:txStyles></p:sldMaster>"#
    );
    let master_rels = rels(&[("rId1", "theme", "../theme/theme1.xml")]);
    let layout = format!(
        r#"<p:sldLayout {NS}><p:cSld><p:spTree>
      <p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="457200" y="200000"/><a:ext cx="8229600" cy="800000"/></a:xfrm></p:spPr><p:txBody><a:bodyPr anchor="b"/><a:p><a:r><a:t>Click to add title</a:t></a:r></a:p></p:txBody></p:sp>
      <p:sp><p:nvSpPr><p:cNvPr id="3" name="Band"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="5000000"/><a:ext cx="9144000" cy="143500"/></a:xfrm><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></p:spPr></p:sp>
    </p:spTree></p:cSld></p:sldLayout>"#
    );
    let layout_rels = rels(&[("rId1", "slideMaster", "../slideMasters/slideMaster1.xml")]);
    let slide_layout = |more: &[(&str, &str, &str)]| {
        let mut entries = vec![("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml")];
        entries.extend_from_slice(more);
        rels(&entries)
    };
    let slide1 = format!(
        r#"<p:sld {NS}><p:cSld><p:spTree>
      <p:sp><p:nvSpPr><p:cNvPr id="2" name="Title 1"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:p><a:r><a:rPr lang="es"/><a:t>Poza P-01</a:t></a:r></a:p></p:txBody></p:sp>
      <p:sp><p:nvSpPr><p:cNvPr id="3" name="Box"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm rot="5400000"><a:off x="100" y="200"/><a:ext cx="3000" cy="4000"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/><a:p><a:pPr algn="r"/><a:r><a:rPr sz="1200" i="1"><a:solidFill><a:schemeClr val="accent1"><a:lumMod val="50000"/></a:schemeClr></a:solidFill></a:rPr><a:t>62.8 m³</a:t></a:r><a:br/><a:r><a:t>TR200</a:t></a:r></a:p></p:txBody></p:sp>
      <p:pic><p:nvPicPr><p:cNvPr id="4" name="Pic"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId2"/></p:blipFill><p:spPr><a:xfrm><a:off x="10" y="20"/><a:ext cx="30" cy="40"/></a:xfrm></p:spPr></p:pic>
      <p:grpSp><p:nvGrpSpPr><p:cNvPr id="5" name="G"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="1000" y="1000"/><a:ext cx="200" cy="200"/><a:chOff x="0" y="0"/><a:chExt cx="100" cy="100"/></a:xfrm></p:grpSpPr>
        <p:sp><p:nvSpPr><p:cNvPr id="6" name="Dot"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="50" y="50"/><a:ext cx="10" cy="10"/></a:xfrm><a:prstGeom prst="ellipse"/><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></p:spPr></p:sp>
      </p:grpSp>
      <p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="7" name="T"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="0" y="3000000"/><a:ext cx="4000000" cy="600000"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table"><a:tbl><a:tblGrid><a:gridCol w="2000000"/><a:gridCol w="2000000"/></a:tblGrid><a:tr h="300000"><a:tc gridSpan="2"><a:txBody><a:bodyPr/><a:p><a:r><a:t>Caudal</a:t></a:r></a:p></a:txBody></a:tc><a:tc hMerge="1"><a:txBody><a:bodyPr/><a:p/></a:txBody></a:tc></a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>
      <p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="8" name="C"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="0" y="0"/><a:ext cx="10" cy="10"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"/></a:graphic></p:graphicFrame>
    </p:spTree></p:cSld></p:sld>"#
    );
    let slide2 = format!(
        r#"<p:sld {NS}><p:cSld><p:bg><p:bgPr><a:solidFill><a:srgbClr val="112233"/></a:solidFill></p:bgPr></p:bg><p:spTree/></p:cSld></p:sld>"#
    );
    let slide3 = format!(r#"<p:sld {NS} show="0"><p:cSld><p:spTree/></p:cSld></p:sld>"#);
    let slide1_rels = slide_layout(&[("rId2", "image", "../media/image1.png")]);
    let slide_rels = slide_layout(&[]);
    let image = png();
    package(&[
        ("ppt/presentation.xml", presentation.as_bytes()),
        (
            "ppt/_rels/presentation.xml.rels",
            presentation_rels.as_bytes(),
        ),
        ("ppt/theme/theme1.xml", theme.as_bytes()),
        ("ppt/slideMasters/slideMaster1.xml", master.as_bytes()),
        (
            "ppt/slideMasters/_rels/slideMaster1.xml.rels",
            master_rels.as_bytes(),
        ),
        ("ppt/slideLayouts/slideLayout1.xml", layout.as_bytes()),
        (
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
            layout_rels.as_bytes(),
        ),
        ("ppt/slides/slide1.xml", slide1.as_bytes()),
        ("ppt/slides/_rels/slide1.xml.rels", slide1_rels.as_bytes()),
        ("ppt/media/image1.png", image.as_slice()),
        ("ppt/slides/slide2.xml", slide2.as_bytes()),
        ("ppt/slides/_rels/slide2.xml.rels", slide_rels.as_bytes()),
        ("ppt/slides/slide3.xml", slide3.as_bytes()),
        ("ppt/slides/_rels/slide3.xml.rels", slide_rels.as_bytes()),
    ])
}

fn shapes(slide: &Slide) -> Vec<&ShapeItem> {
    slide
        .items
        .iter()
        .filter_map(|item| match item {
            SlideItem::Shape(shape) => Some(shape),
            _ => None,
        })
        .collect()
}

#[test]
fn lists_the_slides_in_the_decks_order_without_hidden_ones() {
    let result = parse_pptx(deck()).expect("a deck");
    assert_eq!(result.width, 9_144_000.0);
    assert_eq!(result.height, 5_143_500.0);
    let numbers: Vec<usize> = result.slides.iter().map(|slide| slide.number).collect();
    assert_eq!(numbers, [1, 2]);
    // The first in order is slide2.xml, with a background of its own.
    assert_eq!(result.slides[0].background.as_deref(), Some("#112233"));
    assert_eq!(result.slides[1].background.as_deref(), Some("#fafafa"));
}

#[test]
fn places_a_title_where_its_layout_does_styled_by_its_master() {
    let slide = parse_pptx(deck()).expect("a deck").slides.remove(1);
    let title = shapes(&slide)
        .into_iter()
        .find(|shape| {
            shape
                .text
                .as_ref()
                .and_then(|t| t.paragraphs.first()?.runs.first())
                .is_some_and(|run| run.text == "Poza P-01")
        })
        .expect("a title");
    assert_eq!(
        (title.frame.x, title.frame.y, title.frame.w, title.frame.h),
        (457_200.0, 200_000.0, 8_229_600.0, 800_000.0)
    );
    let text = title.text.as_ref().expect("text");
    assert_eq!(text.anchor, Anchor::Bottom);
    assert_eq!(text.paragraphs[0].align, Align::Center);
    assert_eq!(text.paragraphs[0].size, 40.0);
    assert!(text.paragraphs[0].runs[0].bold);
    assert_eq!(text.paragraphs[0].runs[0].size, 40.0);
    // The layout's own band shows; its prompt text never does.
    assert!(shapes(&slide)
        .iter()
        .any(|shape| shape.fill.as_deref() == Some("#1f6feb")));
    let json = serde_json::to_string(&slide.items).expect("json");
    assert!(!json.contains("Click to add title"));
}

#[test]
fn reads_text_runs_pictures_groups_tables_and_what_it_cannot_draw() {
    let items = parse_pptx(deck()).expect("a deck").slides.remove(1).items;
    let slide = Slide {
        number: 2,
        background: None,
        background_image: None,
        items: items.clone(),
    };
    let boxed = shapes(&slide)
        .into_iter()
        .find(|shape| {
            shape
                .text
                .as_ref()
                .is_some_and(|t| t.paragraphs[0].align == Align::Right)
        })
        .expect("a text box");
    assert_eq!(
        (
            boxed.frame.x,
            boxed.frame.y,
            boxed.frame.w,
            boxed.frame.h,
            boxed.frame.rotate
        ),
        (100.0, 200.0, 3000.0, 4000.0, Some(90.0))
    );
    let text = boxed.text.as_ref().expect("text");
    let runs: Vec<&str> = text.paragraphs[0]
        .runs
        .iter()
        .map(|r| r.text.as_str())
        .collect();
    assert_eq!(runs, ["62.8 m³", "\n", "TR200"]);
    let first = &text.paragraphs[0].runs[0];
    assert_eq!(first.size, 12.0);
    assert!(first.italic);
    assert_eq!(first.color.as_deref(), Some("#103876"));

    let picture = items
        .iter()
        .find_map(|item| match item {
            SlideItem::Image(image) => Some(image),
            _ => None,
        })
        .expect("a picture");
    assert_eq!(
        (
            picture.frame.x,
            picture.frame.y,
            picture.frame.w,
            picture.frame.h
        ),
        (10.0, 20.0, 30.0, 40.0)
    );
    assert_eq!(picture.image.mime, "image/png");
    assert_eq!(picture.image.bytes, png());

    // A group's child, mapped from the group's own space to the slide's.
    let dot = shapes(&slide)
        .into_iter()
        .find(|shape| shape.shape == Some(ShapeGeometry::Ellipse))
        .expect("a dot");
    assert_eq!(
        (dot.frame.x, dot.frame.y, dot.frame.w, dot.frame.h),
        (1100.0, 1100.0, 20.0, 20.0)
    );
    assert_eq!(dot.fill.as_deref(), Some("#ff0000"));

    let table = items
        .iter()
        .find_map(|item| match item {
            SlideItem::Table(table) => Some(table),
            _ => None,
        })
        .expect("a table");
    assert_eq!(table.columns, [2_000_000.0, 2_000_000.0]);
    assert_eq!(table.rows[0].cells.len(), 1);
    assert_eq!(table.rows[0].cells[0].span, 2);
    assert_eq!(
        table.rows[0].cells[0].text.paragraphs[0].runs[0].text,
        "Caudal"
    );

    let unsupported = items
        .iter()
        .find_map(|item| match item {
            SlideItem::Unsupported(item) => Some(item),
            _ => None,
        })
        .expect("a chart card");
    assert_eq!(unsupported.label, "Chart");
}

#[test]
fn serializes_the_way_the_typescript_model_does() {
    let slide = parse_pptx(deck()).expect("a deck").slides.remove(1);
    let json = serde_json::to_value(&slide).expect("json");
    let first = &json["items"][0];
    assert_eq!(first["kind"], "shape");
    assert!(first["x"].is_number());
    assert!(json["items"]
        .as_array()
        .expect("items")
        .iter()
        .any(|i| i["kind"] == "image"));
    let round: Slide = serde_json::from_value(json).expect("round trip");
    assert_eq!(round, slide);
}

#[test]
fn says_when_a_file_is_not_a_deck() {
    let zip = package(&[("word/document.xml", b"<w:document/>")]);
    assert_eq!(
        parse_pptx(zip),
        Err("This is not a PowerPoint deck.".to_string())
    );
    assert!(parse_pptx(b"plainly not a zip".to_vec()).is_err());
}

#[test]
fn fills_a_slide_backdrop_from_its_image_and_bullets_a_body() {
    let presentation = format!(
        r#"<p:presentation {NS}><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst></p:presentation>"#
    );
    let presentation_rels = rels(&[("rId1", "slide", "slides/slide1.xml")]);
    let slide = format!(
        r#"<p:sld {NS}><p:cSld><p:bg><p:bgPr><a:blipFill><a:blip r:embed="rId9"/></a:blipFill></p:bgPr></p:bg><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="2" name="Body"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="100" cy="100"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/><a:lstStyle><a:lvl1pPr><a:buChar char="-"/></a:lvl1pPr></a:lstStyle><a:p><a:r><a:t>uno</a:t></a:r></a:p><a:p><a:pPr><a:buNone/></a:pPr><a:r><a:t>dos</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#
    );
    let slide_rels = rels(&[("rId9", "image", "../media/back.jpg")]);
    let bytes = package(&[
        ("ppt/presentation.xml", presentation.as_bytes()),
        (
            "ppt/_rels/presentation.xml.rels",
            presentation_rels.as_bytes(),
        ),
        ("ppt/slides/slide1.xml", slide.as_bytes()),
        ("ppt/slides/_rels/slide1.xml.rels", slide_rels.as_bytes()),
        ("ppt/media/back.jpg", b"jpeg bytes"),
    ]);
    let deck = parse_pptx(bytes).expect("a deck");
    // No <p:sldSz>: the widescreen default.
    assert_eq!((deck.width, deck.height), (12_192_000.0, 6_858_000.0));
    let slide = &deck.slides[0];
    let backdrop = slide.background_image.as_ref().expect("a backdrop");
    assert_eq!(backdrop.mime, "image/jpeg");
    let body = shapes(slide).remove(0);
    let paragraphs = &body.text.as_ref().expect("text").paragraphs;
    assert_eq!(paragraphs[0].bullet.as_deref(), Some("-"));
    assert_eq!(paragraphs[1].bullet, None);
}

#[test]
fn a_picture_it_cannot_load_becomes_a_labelled_box() {
    let presentation = format!(
        r#"<p:presentation {NS}><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst></p:presentation>"#
    );
    let presentation_rels = rels(&[("rId1", "slide", "slides/slide1.xml")]);
    let slide = format!(
        r#"<p:sld {NS}><p:cSld><p:spTree><p:pic><p:nvPicPr><p:cNvPr id="4" name="Pic"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId2"/></p:blipFill><p:spPr><a:xfrm><a:off x="1" y="2"/><a:ext cx="3" cy="4"/></a:xfrm></p:spPr></p:pic></p:spTree></p:cSld></p:sld>"#
    );
    let slide_rels = rels(&[("rId2", "image", "../media/scan.emf")]);
    let bytes = package(&[
        ("ppt/presentation.xml", presentation.as_bytes()),
        (
            "ppt/_rels/presentation.xml.rels",
            presentation_rels.as_bytes(),
        ),
        ("ppt/slides/slide1.xml", slide.as_bytes()),
        ("ppt/slides/_rels/slide1.xml.rels", slide_rels.as_bytes()),
        ("ppt/media/scan.emf", b"emf"),
    ]);
    let deck = parse_pptx(bytes).expect("a deck");
    assert!(matches!(
        &deck.slides[0].items[0],
        SlideItem::Unsupported(item) if item.label == "Picture"
    ));
}
