//! `cargo xtask icons`: regenerates the icon assets from their npm packages.
//!
//! ```sh
//! # SVGs in crates/monocode-gpui/assets/hugeicons, white strokes at the given width.
//! cargo xtask icons hugeicons <@hugeicons/core-free-icons dir> <out dir> name=Icon@stroke …
//! # The file, extension and folder tables and SVGs of material-icons.json.
//! cargo xtask icons material <react-material-icon-theme dir> <out.json>
//! # SVGs in crates/monocode-gpui/assets/fluent, filled white.
//! cargo xtask icons fluent <@fluentui/svg-icons dir> <out dir> name=file …
//! ```
//!
//! The first two packages ship JavaScript, so QuickJS reads them (the
//! optional `icons` feature; this command turns it on by itself). Fluent's
//! are plain SVG files.
// Without the feature only the re-run and the tests use these helpers.
#![cfg_attr(not(feature = "icons"), allow(dead_code))]
use crate::util::Result;

const USAGE: &str = "usage: cargo xtask icons hugeicons <package dir> <out dir> name=Icon@stroke …\n       cargo xtask icons material <package dir> <out.json>\n       cargo xtask icons fluent <package dir> <out dir> name=file …";

#[cfg(not(feature = "icons"))]
pub fn main(argv: &[String]) -> Result<()> {
    match argv.split_first() {
        Some((kind, rest)) if kind == "fluent" && rest.len() >= 2 => fluent(
            std::path::Path::new(&rest[0]),
            std::path::Path::new(&rest[1]),
            &rest[2..],
        ),
        _ => crate::util::rerun_with_feature("icons", argv),
    }
}

#[cfg(feature = "icons")]
pub fn main(argv: &[String]) -> Result<()> {
    use std::path::Path;
    match argv.split_first() {
        Some((kind, rest)) if kind == "hugeicons" && rest.len() >= 2 => {
            js::hugeicons(Path::new(&rest[0]), Path::new(&rest[1]), &rest[2..])
        }
        Some((kind, rest)) if kind == "material" && rest.len() == 2 => {
            js::material(Path::new(&rest[0]), Path::new(&rest[1]))
        }
        Some((kind, rest)) if kind == "fluent" && rest.len() >= 2 => {
            fluent(Path::new(&rest[0]), Path::new(&rest[1]), &rest[2..])
        }
        _ => Err(USAGE.into()),
    }
}

/// Copies each `name=file` from the package's `icons/<file>.svg` to
/// `<out>/<name>.svg`, filled white.
fn fluent(dir: &std::path::Path, out: &std::path::Path, specs: &[String]) -> Result<()> {
    for spec in specs {
        let (name, file) = spec
            .split_once('=')
            .ok_or_else(|| format!("Expected name=file, found {spec}"))?;
        let path = dir.join("icons").join(format!("{file}.svg"));
        let source =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let svg = fluent_svg(&source).ok_or_else(|| format!("{}: not an SVG", path.display()))?;
        let target = out.join(format!("{name}.svg"));
        std::fs::write(&target, svg).map_err(|e| format!("{}: {e}", target.display()))?;
    }
    Ok(())
}

/// A Fluent icon filled white, as the app tints every icon; its fixed
/// width and height go, so it scales to the box it is drawn in.
fn fluent_svg(source: &str) -> Option<String> {
    let start = source.find("<svg")?;
    let end = start + source[start..].find('>')?;
    let attributes: Vec<&str> = source[start + 4..end]
        .split_whitespace()
        .filter(|a| {
            !a.starts_with("width=") && !a.starts_with("height=") && !a.starts_with("fill=")
        })
        .collect();
    Some(format!(
        "<svg {} fill=\"white\">{}",
        attributes.join(" "),
        source[end + 1..].trim_end()
    ))
}

/// `strokeLinecap` → `stroke-linecap`.
fn attribute(key: &str) -> String {
    let mut name = String::new();
    for c in key.chars() {
        if c.is_ascii_uppercase() {
            name.push('-');
            name.push(c.to_ascii_lowercase());
        } else {
            name.push(c);
        }
    }
    name
}

type Element = (String, Vec<(String, String)>);

/// One SVG from an icon's elements, drawn white at `width`.
fn svg(elements: &[Element], width: &str) -> String {
    let body: String = elements
        .iter()
        .map(|(tag, attributes)| {
            let parts: Vec<String> = attributes
                .iter()
                .filter(|(k, _)| k != "key")
                .map(|(k, v)| {
                    let value = match k.as_str() {
                        "stroke" | "fill" if v == "currentColor" => "white",
                        "strokeWidth" => width,
                        _ => v,
                    };
                    format!("{}=\"{value}\"", attribute(k))
                })
                .collect();
            format!("<{tag} {}/>", parts.join(" "))
        })
        .collect();
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"none\">{body}</svg>")
}

/// The stroke-only FoldVertical/UnfoldVertical the app draws.
fn fold(icon: &str) -> Option<Vec<Element>> {
    let paths: &[&str] = match icon {
        "FoldVertical" => &["M7 2L12 7L17 2", "M7 22L12 17L17 22"],
        "UnfoldVertical" => &["M7 7L12 2L17 7", "M7 17L12 22L17 17"],
        _ => return None,
    };
    let dashes = ["M2 12H4", "M8 12H10", "M14 12H16", "M20 12H22"];
    Some(
        dashes
            .iter()
            .chain(paths)
            .map(|d| {
                let attributes = [
                    ("d", *d),
                    ("stroke", "currentColor"),
                    ("strokeLinecap", "round"),
                    ("strokeLinejoin", "round"),
                    ("strokeWidth", "1.5"),
                ];
                (
                    "path".to_owned(),
                    attributes
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                )
            })
            .collect(),
    )
}

/// Object keys in JavaScript's own order: array indices ascending, then the
/// rest as inserted. `insert` keeps the first value, like `??=`.
#[derive(Default)]
struct JsObject {
    keys: Vec<String>,
    values: std::collections::HashMap<String, String>,
}

impl JsObject {
    fn insert(&mut self, key: &str, value: &str) {
        // `object.__proto__ ??= value` reads the prototype and adds nothing.
        if key != "__proto__" && !self.values.contains_key(key) {
            self.keys.push(key.to_owned());
            self.values.insert(key.to_owned(), value.to_owned());
        }
    }

    fn index(key: &str) -> Option<u32> {
        let canonical = key == "0" || (!key.starts_with('0') && !key.is_empty());
        key.parse::<u32>()
            .ok()
            .filter(|n| canonical && key.bytes().all(|b| b.is_ascii_digit()) && *n < u32::MAX)
    }

    fn ordered(&self) -> Vec<(&str, &str)> {
        let mut indices: Vec<&String> = self
            .keys
            .iter()
            .filter(|k| Self::index(k).is_some())
            .collect();
        indices.sort_by_key(|k| Self::index(k));
        indices
            .into_iter()
            .chain(self.keys.iter().filter(|k| Self::index(k).is_none()))
            .map(|k| (k.as_str(), self.values[k].as_str()))
            .collect()
    }

    fn len(&self) -> usize {
        self.keys.len()
    }

    /// `JSON.stringify` of the object.
    fn json(&self) -> String {
        let quote = |s: &str| serde_json::to_string(s).expect("strings serialize");
        let fields: Vec<String> = self
            .ordered()
            .into_iter()
            .map(|(k, v)| format!("{}:{}", quote(k), quote(v)))
            .collect();
        format!("{{{}}}", fields.join(","))
    }
}

#[cfg(feature = "icons")]
mod js {
    use super::{fold, svg, Element, JsObject};
    use crate::util::Result;
    use rquickjs::{
        convert::Coerced, Array, CatchResultExt, Context, Ctx, Function, Module, Object, Runtime,
        Value,
    };
    use std::path::Path;

    fn with_js<R>(run: impl for<'js> FnOnce(Ctx<'js>) -> rquickjs::Result<R>) -> Result<R> {
        let runtime = Runtime::new().map_err(|e| e.to_string())?;
        let context = Context::full(&runtime).map_err(|e| e.to_string())?;
        context.with(|ctx| {
            run(ctx.clone())
                .catch(&ctx)
                .map_err(|error| error.to_string())
        })
    }

    fn truthy(value: &Value) -> bool {
        if value.is_undefined() || value.is_null() {
            return false;
        }
        if let Some(b) = value.as_bool() {
            return b;
        }
        if let Some(n) = value.as_number() {
            return n != 0.0 && !n.is_nan();
        }
        if let Some(s) = value.as_string() {
            return s.to_string().is_ok_and(|s| !s.is_empty());
        }
        true
    }

    fn strings<'js>(object: &Object<'js>, key: &str) -> rquickjs::Result<Vec<String>> {
        let value: Value = object.get(key)?;
        match value.as_array() {
            Some(array) => array.iter::<String>().collect(),
            None => Ok(Vec::new()),
        }
    }

    pub fn hugeicons(dir: &Path, out: &Path, specs: &[String]) -> Result<()> {
        for spec in specs {
            let (name, rest) = spec
                .split_once('=')
                .ok_or_else(|| format!("Expected name=Icon@stroke, found {spec}"))?;
            let (icon, width) = rest
                .split_once('@')
                .ok_or_else(|| format!("Expected name=Icon@stroke, found {spec}"))?;
            let elements = match fold(icon) {
                Some(elements) => elements,
                None => {
                    let path = dir.join("dist/esm").join(format!("{icon}.js"));
                    let source = std::fs::read_to_string(&path)
                        .map_err(|e| format!("{}: {e}", path.display()))?;
                    with_js(|ctx| {
                        let (module, done) = Module::declare(ctx, icon, source)?.eval()?;
                        done.finish::<()>()?;
                        let list: Array = module.get("default")?;
                        let mut elements: Vec<Element> = Vec::new();
                        for item in list.iter::<Array>() {
                            let item = item?;
                            let attributes: Object = item.get(1)?;
                            let attributes = attributes
                                .props::<String, Coerced<String>>()
                                .map(|p| p.map(|(k, v)| (k, v.0)))
                                .collect::<rquickjs::Result<_>>()?;
                            elements.push((item.get(0)?, attributes));
                        }
                        Ok(elements)
                    })?
                }
            };
            let file = out.join(format!("{name}.svg"));
            std::fs::write(&file, svg(&elements, width))
                .map_err(|e| format!("{}: {e}", file.display()))?;
        }
        Ok(())
    }

    /// Stubs for the React hooks the package imports; tables only need data.
    const REACT: &str = "export default {}; export const useMemo = (f) => f(), useState = (v) => [v, () => {}], useCallback = (f) => f, createContext = () => ({}), useContext = () => ({});";

    pub fn material(dir: &Path, out: &Path) -> Result<()> {
        let path = dir.join("dist/index.esm.js");
        let source =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let (files, exts, folders, svgs) = with_js(|ctx| {
            ctx.eval::<(), _>("globalThis.process = { env: { NODE_ENV: 'production' } };")?;
            let (_, react) = Module::declare(ctx.clone(), "react", REACT)?.eval()?;
            react.finish::<()>()?;
            let (module, done) =
                Module::declare(ctx, "react-material-icon-theme", source)?.eval()?;
            done.finish::<()>()?;
            let mut files = JsObject::default();
            let mut exts = JsObject::default();
            let mut folders = JsObject::default();
            let file_icons: Object = module.get("fileIcons")?;
            let icons: Array = file_icons.get("icons")?;
            for icon in icons.iter::<Object>() {
                let icon = icon?;
                if truthy(&icon.get("disabled")?) || truthy(&icon.get("enabledFor")?) {
                    continue;
                }
                let name: String = icon.get("name")?;
                for file in strings(&icon, "fileNames")? {
                    files.insert(&file, &name);
                }
                for ext in strings(&icon, "fileExtensions")? {
                    exts.insert(&ext, &name);
                }
            }
            let themes: Array = module.get("folderIcons")?;
            let mut specific = None;
            for theme in themes.iter::<Object>() {
                let theme = theme?;
                if theme.get::<_, String>("name")? == "specific" {
                    specific = Some(theme);
                    break;
                }
            }
            let specific = specific.ok_or_else(|| {
                rquickjs::Error::new_from_js_message("folderIcons", "theme", "no specific theme")
            })?;
            let icons: Array = specific.get("icons")?;
            for icon in icons.iter::<Object>() {
                let icon = icon?;
                if truthy(&icon.get("disabled")?) {
                    continue;
                }
                let name: String = icon.get("name")?;
                for folder in strings(&icon, "folderNames")? {
                    let mut keys = vec![
                        folder.clone(),
                        format!(".{folder}"),
                        format!("_{folder}"),
                        format!("-{folder}"),
                        format!("__{folder}__"),
                    ];
                    for prefix in [".", "_", "-"] {
                        if let Some(rest) = folder.strip_prefix(prefix) {
                            keys.push(rest.to_owned());
                        }
                    }
                    if folder.starts_with("__") && folder.ends_with("__") && folder.len() > 4 {
                        keys.push(folder[2..folder.len() - 2].to_owned());
                    }
                    for key in keys {
                        folders.insert(&key, &name);
                    }
                }
            }
            let mut used = JsObject::default();
            for name in ["file", "folder", "folder-open"] {
                used.insert(name, "");
            }
            for (_, name) in files.ordered().into_iter().chain(exts.ordered()) {
                used.insert(name, "");
            }
            for (_, name) in folders.ordered() {
                used.insert(name, "");
                used.insert(&format!("{name}-open"), "");
            }
            let icon_svg: Function = module.get("getIconSvg")?;
            let mut svgs = JsObject::default();
            for (name, _) in used.ordered() {
                let value: Value = icon_svg.call((name,))?;
                if truthy(&value) {
                    svgs.insert(name, &value.get::<Coerced<String>>()?.0);
                }
            }
            Ok((files, exts, folders, svgs))
        })?;
        let json = format!(
            "{{\"files\":{},\"exts\":{},\"folders\":{},\"svgs\":{}}}",
            files.json(),
            exts.json(),
            folders.json(),
            svgs.json()
        );
        std::fs::write(out, json).map_err(|e| format!("{}: {e}", out.display()))?;
        println!(
            "{} {} {} {}",
            files.len(),
            exts.len(),
            folders.len(),
            svgs.len()
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_white_strokes_at_the_requested_width() {
        let elements = vec![(
            "path".to_owned(),
            vec![
                ("d".to_owned(), "M1 1".to_owned()),
                ("stroke".to_owned(), "currentColor".to_owned()),
                ("strokeLinecap".to_owned(), "round".to_owned()),
                ("strokeWidth".to_owned(), "1.5".to_owned()),
                ("key".to_owned(), "0".to_owned()),
            ],
        )];
        assert_eq!(
            svg(&elements, "2"),
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none"><path d="M1 1" stroke="white" stroke-linecap="round" stroke-width="2"/></svg>"#
        );
        assert_eq!(fold("FoldVertical").unwrap().len(), 6);
        assert!(fold("Home01Icon").is_none());
    }

    #[test]
    fn fluent_icons_are_filled_white_and_scale() {
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><path d="M5 5z"/></svg>
"#;
        assert_eq!(
            fluent_svg(source).unwrap(),
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="white"><path d="M5 5z"/></svg>"#
        );
        assert!(fluent_svg("not an icon").is_none());
    }

    #[test]
    fn objects_keep_javascript_key_order() {
        let mut object = JsObject::default();
        for (k, v) in [
            ("b", "1"),
            ("10", "2"),
            ("a", "3"),
            ("2", "4"),
            ("b", "5"),
            ("02", "6"),
        ] {
            object.insert(k, v);
        }
        assert_eq!(
            object.json(),
            r#"{"2":"4","10":"2","b":"1","a":"3","02":"6"}"#
        );
    }
}
