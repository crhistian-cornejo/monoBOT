use super::*;
use crate::office::fixtures::{package, png, rels, REL};

const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#;

const STYLES: &str = r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri"/><w:sz w:val="22"/></w:rPr></w:rPrDefault></w:docDefaults>
  <w:style w:type="paragraph" w:styleId="Normal"><w:name w:val="Normal"/></w:style>
  <w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:pPr><w:jc w:val="center"/></w:pPr><w:rPr><w:b/><w:color w:val="1F3864"/><w:sz w:val="32"/></w:rPr></w:style>
  <w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Heading1"/><w:rPr><w:sz w:val="26"/></w:rPr></w:style>
  <w:style w:type="paragraph" w:styleId="Capitulo"><w:name w:val="Capitulo"/><w:pPr><w:outlineLvl w:val="2"/></w:pPr></w:style>
  <w:style w:type="character" w:styleId="Fuerte"><w:name w:val="Strong"/><w:rPr><w:b/></w:rPr></w:style>
  <w:style w:type="paragraph" w:styleId="Lista"><w:name w:val="List Paragraph"/><w:pPr><w:numPr><w:numId w:val="2"/></w:numPr></w:pPr></w:style>
</w:styles>"#;

const NUMBERING: &str = r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:abstractNum w:abstractNumId="0">
    <w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl>
    <w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%2)"/></w:lvl>
  </w:abstractNum>
  <w:abstractNum w:abstractNumId="1">
    <w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/><w:lvlText w:val="&#xF0B7;"/></w:lvl>
  </w:abstractNum>
  <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
  <w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>
</w:numbering>"#;

fn document_xml() -> String {
    format!(
        r#"<w:document {W}><w:body>
  <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Memoria descriptiva</w:t></w:r></w:p>
  <w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>Alcance</w:t></w:r></w:p>
  <w:p><w:pPr><w:pStyle w:val="Capitulo"/></w:pPr><w:r><w:t>Anexo</w:t></w:r></w:p>
  <w:p>
    <w:r><w:t xml:space="preserve">Poza </w:t></w:r>
    <w:r><w:t>P-01 </w:t></w:r>
    <w:r><w:rPr><w:b/><w:i/><w:color w:val="C00000"/><w:sz w:val="28"/></w:rPr><w:t>62.8 m³</w:t></w:r>
    <w:r><w:rPr><w:rStyle w:val="Fuerte"/><w:vertAlign w:val="superscript"/></w:rPr><w:t>2</w:t></w:r>
    <w:r><w:tab/><w:t>fin</w:t><w:br/><w:t>otra línea</w:t></w:r>
    <w:hyperlink r:id="rId9"><w:r><w:rPr><w:u w:val="single"/></w:rPr><w:t>SENACE</w:t></w:r></w:hyperlink>
  </w:p>
  <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Primero</w:t></w:r></w:p>
  <w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="1"/></w:numPr><w:ind w:left="720"/></w:pPr><w:r><w:t>Uno</w:t></w:r></w:p>
  <w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Dos</w:t></w:r></w:p>
  <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Segundo</w:t></w:r></w:p>
  <w:p><w:pPr><w:pStyle w:val="Lista"/><w:pageBreakBefore/></w:pPr><w:r><w:t>Viñeta</w:t></w:r></w:p>
  <w:p><w:r><w:drawing><wp:inline><wp:extent cx="914400" cy="457200"/><wp:docPr id="1" name="Imagen 1" descr="Plano de planta"/><a:graphic><a:graphicData><a:blip r:embed="rId5"/></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>
  <w:tbl>
    <w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="3000"/><w:gridCol w:w="1000"/></w:tblGrid>
    <w:tr><w:trPr><w:tblHeader/></w:trPr>
      <w:tc><w:tcPr><w:gridSpan w:val="2"/><w:shd w:val="clear" w:fill="D9E2F3"/></w:tcPr><w:p><w:r><w:t>Caudal</w:t></w:r></w:p></w:tc>
      <w:tc><w:p><w:r><w:t>Unidad</w:t></w:r></w:p></w:tc>
    </w:tr>
    <w:tr>
      <w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>TR100</w:t></w:r></w:p></w:tc>
      <w:tc><w:p><w:r><w:t>99</w:t></w:r></w:p></w:tc>
      <w:tc><w:p><w:r><w:t>L/s</w:t></w:r></w:p></w:tc>
    </w:tr>
    <w:tr>
      <w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc>
      <w:tc><w:p><w:r><w:t>120</w:t></w:r></w:p></w:tc>
      <w:tc><w:p><w:r><w:t>L/s</w:t></w:r></w:p></w:tc>
    </w:tr>
  </w:tbl>
  <w:sdt><w:sdtContent><w:p><w:r><w:t>Dentro de un control</w:t></w:r></w:p></w:sdtContent></w:sdt>
  <w:sectPr><w:headerReference w:type="default" r:id="rId7"/><w:pgSz w:w="15840" w:h="12240" w:orient="landscape"/><w:pgMar w:top="1000" w:right="1200" w:bottom="1000" w:left="1200"/></w:sectPr>
</w:body></w:document>"#
    )
}

fn docx() -> Vec<u8> {
    let document = document_xml();
    let document_rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="{REL}/styles" Target="styles.xml"/>
  <Relationship Id="rId2" Type="{REL}/numbering" Target="numbering.xml"/>
  <Relationship Id="rId5" Type="{REL}/image" Target="media/plano.png"/>
  <Relationship Id="rId7" Type="{REL}/header" Target="header1.xml"/>
  <Relationship Id="rId9" Type="{REL}/hyperlink" Target="https://www.senace.gob.pe/" TargetMode="External"/>
</Relationships>"#
    );
    let header = format!(
        r#"<w:hdr {W}><w:p><w:r><w:t>Informe 28</w:t></w:r></w:p><w:p><w:r><w:drawing><wp:inline><wp:extent cx="100" cy="100"/><wp:docPr id="2" name="Logo"/><a:graphic><a:graphicData><a:blip r:embed="rId1"/></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p></w:hdr>"#
    );
    let header_rels = rels(&[("rId1", "image", "media/logo.jpg")]);
    let core = r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Memoria de obra</dc:title></cp:coreProperties>"#;
    let image = png();
    package(&[
        ("word/document.xml", document.as_bytes()),
        ("word/_rels/document.xml.rels", document_rels.as_bytes()),
        ("word/styles.xml", STYLES.as_bytes()),
        ("word/numbering.xml", NUMBERING.as_bytes()),
        ("word/header1.xml", header.as_bytes()),
        ("word/_rels/header1.xml.rels", header_rels.as_bytes()),
        ("word/media/plano.png", image.as_slice()),
        ("word/media/logo.jpg", b"jpeg"),
        ("docProps/core.xml", core.as_bytes()),
    ])
}

fn paragraphs(blocks: &[Block]) -> Vec<&Paragraph> {
    blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            Block::Table(_) => None,
        })
        .collect()
}

fn text(paragraph: &Paragraph) -> String {
    paragraph.runs.iter().map(|run| run.text.as_str()).collect()
}

#[test]
fn reads_headings_from_their_styles_and_inherits_formatting() {
    let document = parse_docx(docx()).expect("a document");
    assert_eq!(document.title.as_deref(), Some("Memoria de obra"));
    let all = paragraphs(&document.blocks);
    assert_eq!(text(all[0]), "Memoria descriptiva");
    assert_eq!(all[0].heading, Some(1));
    assert_eq!(all[0].align, Some(Align::Center));
    assert_eq!(all[0].style.as_deref(), Some("Heading1"));
    let first = &all[0].runs[0];
    assert!(first.bold);
    assert_eq!(first.size, Some(16.0));
    assert_eq!(first.color.as_deref(), Some("#1f3864"));
    // Heading 2 is based on Heading 1: it keeps the centre and the colour, but not the size.
    assert_eq!(all[1].heading, Some(2));
    assert_eq!(all[1].align, Some(Align::Center));
    assert_eq!(all[1].runs[0].size, Some(13.0));
    assert_eq!(all[1].runs[0].color.as_deref(), Some("#1f3864"));
    // A custom style outlined at level 3 is a heading too.
    assert_eq!(all[2].heading, Some(3));
}

#[test]
fn reads_runs_with_formatting_links_tabs_and_line_breaks() {
    let document = parse_docx(docx()).expect("a document");
    let body = paragraphs(&document.blocks)[3];
    // Two plain runs with the same look are one.
    assert_eq!(body.runs[0].text, "Poza P-01 ");
    assert!(!body.runs[0].bold);
    assert_eq!(body.runs[0].size, Some(11.0));
    assert_eq!(body.runs[0].font.as_deref(), Some("Calibri"));
    let formatted = &body.runs[1];
    assert_eq!(formatted.text, "62.8 m³");
    assert!(formatted.bold && formatted.italic);
    assert_eq!(formatted.color.as_deref(), Some("#c00000"));
    assert_eq!(formatted.size, Some(14.0));
    let sup = &body.runs[2];
    assert!(sup.bold, "the character style's bold applies");
    assert_eq!(sup.vertical_align, Some(VerticalAlign::Superscript));
    assert_eq!(body.runs[3].text, "\tfin\notra línea");
    let link = body.runs.last().expect("a link");
    assert_eq!(link.text, "SENACE");
    assert!(link.underline);
    assert_eq!(link.link.as_deref(), Some("https://www.senace.gob.pe/"));
}

#[test]
fn numbers_lists_and_nests_them() {
    let document = parse_docx(docx()).expect("a document");
    let all = paragraphs(&document.blocks);
    let marker = |at: usize| {
        all[at]
            .list
            .as_ref()
            .map(|l| (l.level, l.ordered, l.marker.as_str()))
    };
    assert_eq!(marker(4), Some((0, true, "1.")));
    assert_eq!(marker(5), Some((1, true, "a)")));
    assert_eq!(marker(6), Some((1, true, "b)")));
    // Back up a level: the deeper count starts over after the next item.
    assert_eq!(marker(7), Some((0, true, "2.")));
    assert_eq!(all[5].indent, Some(36.0));
    // A bullet from a style, its symbol-font character swapped for one that draws.
    assert_eq!(marker(8), Some((0, false, "•")));
    assert!(all[8].page_break_before);
}

#[test]
fn reads_pictures_with_their_size_and_description() {
    let document = parse_docx(docx()).expect("a document");
    let picture = paragraphs(&document.blocks)[9];
    let image = picture.runs[0].image.as_ref().expect("an image");
    assert_eq!(image.mime, "image/png");
    assert_eq!(image.bytes, png());
    assert_eq!((image.width, image.height), (914_400.0, 457_200.0));
    assert_eq!(image.alt.as_deref(), Some("Plano de planta"));
}

#[test]
fn reads_tables_with_spans_merges_and_shading() {
    let document = parse_docx(docx()).expect("a document");
    let table = document
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Table(table) => Some(table),
            _ => None,
        })
        .expect("a table");
    assert_eq!(table.columns, [100.0, 150.0, 50.0]);
    assert_eq!(table.rows.len(), 3);
    assert!(table.rows[0].header);
    assert_eq!(table.rows[0].cells.len(), 2);
    assert_eq!(table.rows[0].cells[0].col_span, 2);
    assert_eq!(table.rows[0].cells[0].fill.as_deref(), Some("#d9e2f3"));
    // The restarted cell spans the row below; the continuation is not a cell of its own.
    assert_eq!(table.rows[1].cells[0].row_span, 2);
    assert_eq!(table.rows[2].cells.len(), 2);
    let Block::Paragraph(cell_text) = &table.rows[2].cells[0].blocks[0] else {
        panic!("a paragraph");
    };
    assert_eq!(text(cell_text), "120");
}

#[test]
fn reads_content_controls_the_page_and_the_header() {
    let document = parse_docx(docx()).expect("a document");
    let last = paragraphs(&document.blocks).pop().expect("a paragraph");
    assert_eq!(text(last), "Dentro de un control");
    let page = document.page.expect("a page");
    // Landscape letter in points, margins from twips.
    assert_eq!((page.width, page.height), (792.0, 612.0));
    assert_eq!((page.margin_top, page.margin_left), (50.0, 60.0));
    let header = paragraphs(&document.header);
    assert_eq!(text(header[0]), "Informe 28");
    // The header's picture resolves against the header's own relationships.
    let logo = header[1].runs[0].image.as_ref().expect("a logo");
    assert_eq!(logo.mime, "image/jpeg");
    assert!(document.footer.is_empty());
    assert!(!document.truncated);
}

#[test]
fn serializes_for_a_view() {
    let document = parse_docx(docx()).expect("a document");
    let json = serde_json::to_value(&document).expect("json");
    assert_eq!(json["blocks"][0]["kind"], "paragraph");
    assert_eq!(json["blocks"][0]["heading"], 1);
    let round: DocxDocument = serde_json::from_value(json).expect("round trip");
    assert_eq!(round, document);
}

#[test]
fn says_when_a_file_is_not_a_document() {
    let zip = package(&[("ppt/presentation.xml", b"<p:presentation/>")]);
    assert_eq!(
        parse_docx(zip),
        Err("This is not a Word document.".to_string())
    );
    assert!(parse_docx(b"not a zip".to_vec()).is_err());
}

#[test]
fn counts_letters_and_romans_as_word_does() {
    assert_eq!(counter_text("lowerLetter", 1), "a");
    assert_eq!(counter_text("lowerLetter", 26), "z");
    assert_eq!(counter_text("lowerLetter", 27), "aa");
    assert_eq!(counter_text("upperLetter", 28), "BB");
    assert_eq!(counter_text("lowerRoman", 14), "xiv");
    assert_eq!(counter_text("upperRoman", 1994), "MCMXCIV");
    assert_eq!(counter_text("decimalZero", 3), "03");
}
