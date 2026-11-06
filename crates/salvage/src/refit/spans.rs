//! A small JSON reader that remembers where every value sits in the text, so
//! `.uplugin` edits can touch one array and leave every other byte alone.
//! It accepts the comments and trailing commas Unreal's own reader tolerates.

/// A value and the byte range it covers (`start..end`).
#[derive(Clone, Debug)]
pub struct Spanned {
    pub start: usize,
    pub end: usize,
    pub value: Node,
}

#[derive(Clone, Debug)]
pub enum Node {
    Object(Vec<(String, Spanned)>),
    Array(Vec<Spanned>),
    Str(String),
    /// Numbers, booleans and null: kept as their text.
    Other,
}

impl Spanned {
    pub fn get(&self, key: &str) -> Option<&Spanned> {
        match &self.value {
            Node::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match &self.value {
            Node::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn items(&self) -> &[Spanned] {
        match &self.value {
            Node::Array(items) => items,
            _ => &[],
        }
    }

    /// The strings in an array (other items are skipped).
    pub fn strings(&self) -> Vec<&str> {
        self.items().iter().filter_map(Spanned::as_str).collect()
    }
}

pub fn parse(text: &str) -> Result<Spanned, String> {
    let mut p = Parser {
        b: text.as_bytes(),
        i: 0,
    };
    p.skip();
    let value = p.value()?;
    p.skip();
    if p.i != p.b.len() {
        return Err(p.error("unexpected text after the end"));
    }
    Ok(value)
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn error(&self, what: &str) -> String {
        let line = self.b[..self.i.min(self.b.len())]
            .iter()
            .filter(|&&c| c == b'\n')
            .count()
            + 1;
        format!("{what} on line {line}")
    }

    /// Skips whitespace and comments.
    fn skip(&mut self) {
        loop {
            while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            if self.b[self.i..].starts_with(b"//") {
                while self.i < self.b.len() && self.b[self.i] != b'\n' {
                    self.i += 1;
                }
            } else if self.b[self.i..].starts_with(b"/*") {
                self.i += 2;
                while self.i < self.b.len() && !self.b[self.i..].starts_with(b"*/") {
                    self.i += 1;
                }
                self.i = (self.i + 2).min(self.b.len());
            } else {
                return;
            }
        }
    }

    fn value(&mut self) -> Result<Spanned, String> {
        let start = self.i;
        let value = match self.b.get(self.i) {
            Some(b'{') => self.object()?,
            Some(b'[') => self.array()?,
            Some(b'"') => Node::Str(self.string()?),
            Some(_) => {
                while self.i < self.b.len()
                    && !matches!(self.b[self.i], b',' | b']' | b'}' | b'/')
                    && !self.b[self.i].is_ascii_whitespace()
                {
                    self.i += 1;
                }
                if self.i == start {
                    return Err(self.error("expected a value"));
                }
                Node::Other
            }
            None => return Err(self.error("unexpected end")),
        };
        Ok(Spanned {
            start,
            end: self.i,
            value,
        })
    }

    fn object(&mut self) -> Result<Node, String> {
        self.i += 1;
        let mut entries = Vec::new();
        loop {
            self.skip();
            match self.b.get(self.i) {
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Node::Object(entries));
                }
                Some(b'"') => {}
                _ => return Err(self.error("expected a key")),
            }
            let key = self.string()?;
            self.skip();
            if self.b.get(self.i) != Some(&b':') {
                return Err(self.error("expected ':'"));
            }
            self.i += 1;
            self.skip();
            entries.push((key, self.value()?));
            self.skip();
            match self.b.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b'}') => {}
                _ => return Err(self.error("expected ',' or '}'")),
            }
        }
    }

    fn array(&mut self) -> Result<Node, String> {
        self.i += 1;
        let mut items = Vec::new();
        loop {
            self.skip();
            if self.b.get(self.i) == Some(&b']') {
                self.i += 1;
                return Ok(Node::Array(items));
            }
            items.push(self.value()?);
            self.skip();
            match self.b.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b']') => {}
                _ => return Err(self.error("expected ',' or ']'")),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.i += 1;
        let mut out = Vec::new();
        while let Some(&c) = self.b.get(self.i) {
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).map_err(|_| self.error("invalid UTF-8")),
                b'\\' => {
                    let Some(&e) = self.b.get(self.i) else { break };
                    self.i += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'u' => {
                            let hex = self.b.get(self.i..self.i + 4).unwrap_or_default();
                            let code = std::str::from_utf8(hex)
                                .ok()
                                .and_then(|h| u32::from_str_radix(h, 16).ok())
                                .and_then(char::from_u32)
                                .unwrap_or('\u{fffd}');
                            self.i += 4;
                            let mut buf = [0; 4];
                            out.extend_from_slice(code.encode_utf8(&mut buf).as_bytes());
                        }
                        other => out.push(other),
                    }
                }
                other => out.push(other),
            }
        }
        Err(self.error("unterminated string"))
    }
}

/// One text replacement: `range` becomes `with`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub with: String,
}

/// Applies edits that don't overlap, from the end backwards.
pub fn apply(text: &str, mut edits: Vec<Edit>) -> String {
    edits.sort_by_key(|e| std::cmp::Reverse(e.start));
    let mut out = text.to_string();
    for edit in edits {
        out.replace_range(edit.start..edit.end, &edit.with);
    }
    out
}

/// An edit that appends `"<item>"` to a non-empty string array, matching its
/// layout: on its own line with the same indent when the array spans lines.
pub fn append_string(text: &str, array: &Spanned, item: &str) -> Option<Edit> {
    let last = array.items().last()?;
    let inner = &text[array.start..array.end];
    let with = if inner.contains('\n') {
        let line_start = text[..last.start].rfind('\n').map_or(0, |i| i + 1);
        let indent: String = text[line_start..last.start]
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect();
        format!(",\n{indent}\"{item}\"")
    } else {
        format!(", \"{item}\"")
    };
    Some(Edit {
        start: last.end,
        end: last.end,
        with,
    })
}

/// An edit that takes one item out of an array, with the comma that joined it.
pub fn remove_item(array: &Spanned, index: usize) -> Option<Edit> {
    let items = array.items();
    let item = items.get(index)?;
    if items.len() == 1 {
        // The only item: leave an empty array, keeping the brackets.
        return Some(Edit {
            start: array.start + 1,
            end: array.end - 1,
            with: String::new(),
        });
    }
    Some(if index + 1 < items.len() {
        // Up to the start of the next item.
        Edit {
            start: item.start,
            end: items[index + 1].start,
            with: String::new(),
        }
    } else {
        // The last item: from the end of the one before it.
        let previous = &items[index - 1];
        Edit {
            start: previous.end,
            end: item.end,
            with: String::new(),
        }
    })
}
