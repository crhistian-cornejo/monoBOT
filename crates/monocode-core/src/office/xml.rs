//! The XML and zip plumbing every Office reader shares: a tolerant tree
//! keyed by local names (the prefixes `p:`, `a:`, `w:`, `r:` vary between
//! writers), the package's parts, and its relationships.
use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::Path;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use zip::ZipArchive;

/// A part larger than this is not read: it is a zip bomb or not a document.
const MAX_PART_BYTES: u64 = 256 * 1024 * 1024;

/// One element: its local name, attributes, children and direct text.
#[derive(Debug, Clone, Default)]
pub struct XmlNode {
    /// The local name: `c:barChart` becomes `barChart`.
    pub name: String,
    /// Every attribute under its full name (`r:id`) and, unless a plain
    /// attribute of that name exists, under its local name (`id`).
    pub attrs: HashMap<String, String>,
    pub children: Vec<XmlNode>,
    /// The element's own text, entities decoded; children's text excluded.
    pub text: String,
}

fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

impl XmlNode {
    pub fn child(&self, name: &str) -> Option<&XmlNode> {
        self.children.iter().find(|entry| entry.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a XmlNode> {
        self.children.iter().filter(move |entry| entry.name == name)
    }

    /// The first node by that name anywhere under this one, depth first.
    pub fn find(&self, name: &str) -> Option<&XmlNode> {
        for entry in &self.children {
            if entry.name == name {
                return Some(entry);
            }
            if let Some(deeper) = entry.find(name) {
                return Some(deeper);
            }
        }
        None
    }

    /// Every node by that name under this one, in document order.
    pub fn find_all<'a>(&'a self, name: &str) -> Vec<&'a XmlNode> {
        let mut out = Vec::new();
        self.collect(name, &mut out);
        out
    }

    fn collect<'a>(&'a self, name: &str, out: &mut Vec<&'a XmlNode>) {
        for entry in &self.children {
            if entry.name == name {
                out.push(entry);
            }
            entry.collect(name, out);
        }
    }

    /// Follow a chain of child names.
    pub fn path(&self, names: &[&str]) -> Option<&XmlNode> {
        let mut current = self;
        for name in names {
            current = current.child(name)?;
        }
        Some(current)
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(String::as_str)
    }

    /// A relationship attribute (`r:id`, `r:embed`), never a plain one by
    /// that name: `<p:sldId id="256" r:id="rId2"/>` holds both.
    pub fn rel_attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| {
                key.contains(':') && !key.starts_with("xmlns") && local_name(key) == name
            })
            .map(|(_, value)| value.as_str())
    }

    /// This element's text and its descendants', in document order.
    pub fn text_content(&self) -> String {
        let mut out = self.text.clone();
        for entry in &self.children {
            out.push_str(&entry.text_content());
        }
        out
    }

    pub fn number(&self, name: &str) -> Option<f64> {
        number(self.attr(name))
    }
}

/// A finite number out of an attribute, or nothing.
pub fn number(value: Option<&str>) -> Option<f64> {
    let text = value?.trim();
    if text.is_empty() {
        return None;
    }
    text.parse::<f64>().ok().filter(|parsed| parsed.is_finite())
}

pub fn decode_reference(content: &str) -> Option<String> {
    match content {
        "amp" => Some("&".into()),
        "lt" => Some("<".into()),
        "gt" => Some(">".into()),
        "quot" => Some("\"".into()),
        "apos" => Some("'".into()),
        _ => {
            let digits = content.strip_prefix('#')?;
            let code = match digits.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => digits.parse::<u32>().ok()?,
            };
            char::from_u32(code).map(String::from)
        }
    }
}

/// `&amp;`, `&#38;`, `&#x26;` in an attribute value; unknown references stay.
pub fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        match tail.find(';').filter(|end| *end <= 10) {
            Some(end) => match decode_reference(&tail[..end]) {
                Some(decoded) => {
                    out.push_str(&decoded);
                    rest = &tail[end + 1..];
                }
                None => {
                    out.push('&');
                    rest = tail;
                }
            },
            None => {
                out.push('&');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

fn element(start: &BytesStart<'_>) -> XmlNode {
    let name = start.name().as_ref().to_string();
    let mut attrs = HashMap::new();
    for attribute in start.attributes().with_checks(false).flatten() {
        let key = attribute.key.as_ref().to_string();
        let value = decode_entities(&attribute.value);
        let local = local_name(&key).to_string();
        attrs.entry(local).or_insert_with(|| value.clone());
        attrs.insert(key, value);
    }
    XmlNode {
        name: local_name(&name).to_string(),
        attrs,
        children: Vec::new(),
        text: String::new(),
    }
}

/// A tolerant reader for the well-formed XML Office writes. Malformed input
/// yields whatever was read before the break, never an error. The returned
/// node is a synthetic root whose children are the document's elements.
pub fn parse_xml(xml: &str) -> XmlNode {
    let xml = xml.strip_prefix('\u{feff}').unwrap_or(xml);
    let mut reader = Reader::from_str(xml);
    let config = reader.config_mut();
    config.check_end_names = false;
    config.allow_unmatched_ends = true;
    let mut stack = vec![XmlNode {
        name: "#root".into(),
        ..XmlNode::default()
    }];
    let close = |stack: &mut Vec<XmlNode>| {
        if stack.len() > 1 {
            let done = stack.pop().expect("a node above the root");
            stack.last_mut().expect("the root").children.push(done);
        }
    };
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => stack.push(element(&start)),
            Ok(Event::Empty(start)) => {
                let node = element(&start);
                stack.last_mut().expect("the root").children.push(node);
            }
            Ok(Event::End(_)) => close(&mut stack),
            Ok(Event::Text(text)) => {
                // The text is raw; references come as events of their own.
                stack.last_mut().expect("the root").text.push_str(&text);
            }
            Ok(Event::CData(data)) => {
                stack.last_mut().expect("the root").text.push_str(&data);
            }
            Ok(Event::GeneralRef(reference)) => {
                if let Some(decoded) = decode_reference(&reference) {
                    stack.last_mut().expect("the root").text.push_str(&decoded);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => {}
        }
    }
    while stack.len() > 1 {
        close(&mut stack);
    }
    stack.pop().expect("the root")
}

/// The first element of a parsed document, whatever its prefix: `<w:document>`.
pub fn document_element(root: XmlNode) -> Option<XmlNode> {
    root.children.into_iter().next()
}

// ---------------------------------------------------------------- package parts

/// Where a relationship's target lives in the package.
pub fn resolve_part(base: &str, target: &str) -> String {
    if let Some(absolute) = target.strip_prefix('/') {
        return absolute.to_string();
    }
    let mut parts: Vec<&str> = base.split('/').collect();
    parts.pop();
    for piece in target.split('/') {
        match piece {
            ".." => {
                parts.pop();
            }
            "" | "." => {}
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// The path of the relationships part that belongs to a part.
pub fn rels_path(part: &str) -> String {
    match part.rfind('/') {
        Some(slash) => format!("{}_rels/{}.rels", &part[..=slash], &part[slash + 1..]),
        None => format!("_rels/{part}.rels"),
    }
}

#[derive(Debug, Clone)]
pub struct Rel {
    pub target: String,
    /// The relationship's full type URL.
    pub kind: String,
    pub external: bool,
}

impl Rel {
    /// The last segment of the type: `.../relationships/slide` is `slide`.
    pub fn kind_name(&self) -> &str {
        self.kind.rsplit('/').next().unwrap_or("")
    }
}

/// A part's relationships, in the order the file lists them.
#[derive(Debug, Clone, Default)]
pub struct Rels(Vec<(String, Rel)>);

impl Rels {
    pub fn parse(xml: Option<&str>) -> Rels {
        let Some(xml) = xml else {
            return Rels::default();
        };
        let root = parse_xml(xml);
        Rels(
            root.find_all("Relationship")
                .into_iter()
                .filter_map(|rel| {
                    Some((
                        rel.attr("Id")?.to_string(),
                        Rel {
                            target: rel.attr("Target").unwrap_or("").to_string(),
                            kind: rel.attr("Type").unwrap_or("").to_string(),
                            external: rel.attr("TargetMode") == Some("External"),
                        },
                    ))
                })
                .collect(),
        )
    }

    pub fn get(&self, id: &str) -> Option<&Rel> {
        self.0.iter().find(|(key, _)| key == id).map(|(_, rel)| rel)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Rel> {
        self.0.iter().map(|(_, rel)| rel)
    }

    pub fn iter_with_ids(&self) -> impl Iterator<Item = (&str, &Rel)> {
        self.0.iter().map(|(id, rel)| (id.as_str(), rel))
    }
}

/// A zip package opened in memory, its parts read on demand.
pub struct Package {
    archive: ZipArchive<Cursor<Vec<u8>>>,
}

impl Package {
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Package, String> {
        ZipArchive::new(Cursor::new(bytes))
            .map(|archive| Package { archive })
            .map_err(|error| error.to_string())
    }

    pub fn open(path: &Path) -> Result<Package, String> {
        let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
        Package::from_bytes(bytes)
    }

    pub fn has(&self, name: &str) -> bool {
        self.archive.index_for_name(name).is_some()
    }

    pub fn bytes(&mut self, name: &str) -> Option<Vec<u8>> {
        let file = self.archive.by_name(name).ok()?;
        let mut out = Vec::new();
        file.take(MAX_PART_BYTES).read_to_end(&mut out).ok()?;
        Some(out)
    }

    pub fn text(&mut self, name: &str) -> Option<String> {
        self.bytes(name)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    }

    /// A part parsed as XML: its root's first element.
    pub fn xml(&mut self, name: &str) -> Option<XmlNode> {
        let text = self.text(name)?;
        document_element(parse_xml(&text))
    }

    pub fn rels(&mut self, part: &str) -> Rels {
        Rels::parse(self.text(&rels_path(part)).as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_local_names_attributes_entities_and_text() {
        let root = parse_xml(
            "<?xml version=\"1.0\"?><c:a r:id=\"rId1\" val='x &amp; y'><c:b>1 &lt; 2</c:b><c:c/></c:a>",
        );
        let a = &root.children[0];
        assert_eq!(a.name, "a");
        assert_eq!(a.attr("r:id"), Some("rId1"));
        assert_eq!(a.attr("id"), Some("rId1"));
        assert_eq!(a.attr("val"), Some("x & y"));
        let names: Vec<_> = a.children.iter().map(|node| node.name.as_str()).collect();
        assert_eq!(names, ["b", "c"]);
        assert_eq!(a.children[0].text, "1 < 2");
    }

    #[test]
    fn keeps_the_plain_attribute_beside_a_relationship_one() {
        for xml in [
            "<s id=\"257\" r:id=\"rId2\"/>",
            "<s r:id=\"rId2\" id=\"257\"/>",
        ] {
            let root = parse_xml(xml);
            let node = &root.children[0];
            assert_eq!(node.attr("id"), Some("257"));
            assert_eq!(node.rel_attr("id"), Some("rId2"));
        }
    }

    #[test]
    fn reads_character_references_and_cdata() {
        let root = parse_xml("<a>&#65;&#x42;<![CDATA[<c>]]></a>");
        assert_eq!(root.children[0].text, "AB<c>");
    }

    #[test]
    fn survives_broken_markup() {
        let root = parse_xml("<a><b>text</a><c>");
        assert_eq!(root.children[0].name, "a");
        assert_eq!(root.children[0].children[0].text, "text");
    }

    #[test]
    fn resolves_package_parts() {
        assert_eq!(
            resolve_part("xl/worksheets/sheet1.xml", "../drawings/drawing1.xml"),
            "xl/drawings/drawing1.xml"
        );
        assert_eq!(
            resolve_part("xl/workbook.xml", "/xl/worksheets/sheet2.xml"),
            "xl/worksheets/sheet2.xml"
        );
        assert_eq!(
            resolve_part("ppt/slides/slide1.xml", "../media/a.png"),
            "ppt/media/a.png"
        );
        assert_eq!(
            rels_path("xl/worksheets/sheet1.xml"),
            "xl/worksheets/_rels/sheet1.xml.rels"
        );
    }
}
