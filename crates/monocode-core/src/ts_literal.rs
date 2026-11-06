//! Reads plain TypeScript literals (strings, booleans, numbers, arrays and
//! objects) so tests can compare Rust tables with the Tauri sources they port.
use serde_json::{Map, Value};

/// The value assigned to `const <name>` in `source`.
pub(crate) fn parse_const(source: &str, name: &str) -> Value {
    let start = source
        .find(&format!("const {name}"))
        .unwrap_or_else(|| panic!("const {name} not found"));
    let equals = start
        + source[start..]
            .find('=')
            .unwrap_or_else(|| panic!("const {name} has no value"));
    let mut parser = Parser {
        text: &source[equals + 1..],
        at: 0,
    };
    parser.value()
}

struct Parser<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Parser<'a> {
    fn rest(&self) -> &'a str {
        &self.text[self.at..]
    }

    fn skip(&mut self) {
        loop {
            let rest = self.rest();
            let trimmed = rest.trim_start();
            self.at += rest.len() - trimmed.len();
            if trimmed.starts_with("//") {
                self.at += trimmed.find('\n').unwrap_or(trimmed.len());
            } else if trimmed.starts_with("/*") {
                self.at += trimmed.find("*/").expect("unclosed comment") + 2;
            } else {
                return;
            }
        }
    }

    fn eat(&mut self, token: &str) -> bool {
        self.skip();
        let found = self.rest().starts_with(token);
        if found {
            self.at += token.len();
        }
        found
    }

    fn value(&mut self) -> Value {
        self.skip();
        let first = self.rest().chars().next().expect("unexpected end");
        match first {
            '"' | '\'' => {
                let mut text = self.string();
                // `"a" + "b"` splits a long description across lines.
                while self.eat("+") {
                    self.skip();
                    text.push_str(&self.string());
                }
                Value::String(text)
            }
            '[' => {
                self.at += 1;
                let mut items = Vec::new();
                while !self.eat("]") {
                    items.push(self.value());
                    self.eat(",");
                }
                Value::Array(items)
            }
            '{' => {
                self.at += 1;
                let mut object = Map::new();
                while !self.eat("}") {
                    self.skip();
                    let key = if self.rest().starts_with(['"', '\'']) {
                        self.string()
                    } else {
                        self.word()
                    };
                    assert!(self.eat(":"), "expected `:` after {key}");
                    let value = self.value();
                    object.insert(key, value);
                    self.eat(",");
                }
                Value::Object(object)
            }
            _ => match self.word().as_str() {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                "null" | "undefined" => Value::Null,
                word => word
                    .parse::<f64>()
                    .ok()
                    .and_then(serde_json::Number::from_f64)
                    .map(Value::Number)
                    .unwrap_or_else(|| panic!("unsupported literal `{word}`")),
            },
        }
    }

    fn word(&mut self) -> String {
        let rest = self.rest();
        let len = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '.'))
            .unwrap_or(rest.len());
        assert!(len > 0, "unexpected `{}`", &rest[..rest.len().min(20)]);
        self.at += len;
        rest[..len].to_string()
    }

    fn string(&mut self) -> String {
        let mut chars = self.rest().char_indices();
        let (_, quote) = chars.next().unwrap();
        let mut out = String::new();
        while let Some((index, c)) = chars.next() {
            match c {
                '\\' => {
                    let (_, escaped) = chars.next().expect("unfinished escape");
                    out.push(match escaped {
                        'n' => '\n',
                        't' => '\t',
                        other => other,
                    });
                }
                c if c == quote => {
                    self.at += index + 1;
                    return out;
                }
                c => out.push(c),
            }
        }
        panic!("unclosed string");
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn reads_nested_literals_with_comments_and_trailing_commas() {
        let source = r#"
            export const THINGS: readonly Thing[] = [
              {
                id: "a", // the first
                description:
                  "two " + 'parts',
                /* flags */ on: true,
                list: ["x", "y",],
                nested: { script: "src/run.mjs" },
              },
            ];
        "#;
        assert_eq!(
            super::parse_const(source, "THINGS"),
            json!([{
                "id": "a",
                "description": "two parts",
                "on": true,
                "list": ["x", "y"],
                "nested": {"script": "src/run.mjs"},
            }])
        );
    }
}
