//! The XML subset the game writes for saves, parsed into a flat pre-order
//! arena that borrows from the source text.
//!
//! Elements are either `<tag attr="..."/>` or `<tag attr="...">children</tag>`.
//! There is no text content, no comments beyond the `<?xml ...?>` prolog,
//! and attribute values are double-quoted with the five predefined entities
//! (`&amp;` occurs in names such as `&mChart[0][0]`). Element tags name the
//! value type: `class`, `classref`, `array`, `bool`, `u8`..`u64`, `s8`..`s32`,
//! `f32`, `string`, `vector3` (attributes `x y z`) and `time`.

use crate::Error;
use std::borrow::Cow;
use std::str::FromStr;

#[derive(Debug, Clone, Copy)]
struct Raw<'a> {
    tag: &'a str,
    /// Attribute text between the tag name and `>` / `/>`.
    attrs: &'a str,
    /// One past the index of the last descendant.
    end: u32,
}

/// A parsed save XML document.
#[derive(Debug, Clone)]
pub struct Document<'a> {
    nodes: Vec<Raw<'a>>,
}

fn xml_err(offset: usize, message: &'static str) -> Error {
    Error::Xml { offset, message }
}

impl<'a> Document<'a> {
    pub fn parse(src: &'a str) -> Result<Document<'a>, Error> {
        let b = src.as_bytes();
        let mut i = 0;
        let skip_ws = |i: &mut usize| {
            while *i < b.len() && b[*i].is_ascii_whitespace() {
                *i += 1;
            }
        };
        skip_ws(&mut i);
        if src[i..].starts_with("<?") {
            i = src[i..].find("?>").map(|p| i + p + 2).ok_or(xml_err(i, "unterminated prolog"))?;
        }
        let mut nodes: Vec<Raw<'a>> = Vec::new();
        let mut open: Vec<usize> = Vec::new();
        loop {
            skip_ws(&mut i);
            if i >= b.len() {
                break;
            }
            if b[i] != b'<' {
                return Err(xml_err(i, "text content is not expected"));
            }
            if b.get(i + 1) == Some(&b'/') {
                let gt = src[i..].find('>').map(|p| i + p).ok_or(xml_err(i, "unterminated end tag"))?;
                let tag = src[i + 2..gt].trim_end();
                let top = open.pop().ok_or(xml_err(i, "end tag without start tag"))?;
                if nodes[top].tag != tag {
                    return Err(xml_err(i, "end tag does not match start tag"));
                }
                nodes[top].end = nodes.len() as u32;
                i = gt + 1;
                if open.is_empty() {
                    break;
                }
                continue;
            }
            if open.is_empty() && !nodes.is_empty() {
                return Err(xml_err(i, "more than one root element"));
            }
            // Find the closing '>' outside quotes.
            let mut j = i + 1;
            let mut quoted = false;
            while j < b.len() && (quoted || b[j] != b'>') {
                if b[j] == b'"' {
                    quoted = !quoted;
                }
                j += 1;
            }
            if j >= b.len() {
                return Err(xml_err(i, "unterminated start tag"));
            }
            let self_closing = b[j - 1] == b'/';
            let inner = &src[i + 1..if self_closing { j - 1 } else { j }];
            let name_end = inner.find(|c: char| c.is_ascii_whitespace()).unwrap_or(inner.len());
            let tag = &inner[..name_end];
            if tag.is_empty() || !tag.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
                return Err(xml_err(i, "bad element name"));
            }
            let attrs = inner[name_end..].trim();
            check_attrs(attrs).map_err(|m| xml_err(i, m))?;
            let index = nodes.len();
            nodes.push(Raw { tag, attrs, end: index as u32 + 1 });
            i = j + 1;
            if !self_closing {
                open.push(index);
            } else if open.is_empty() {
                break;
            }
        }
        if !open.is_empty() {
            return Err(xml_err(b.len(), "unclosed element"));
        }
        if nodes.is_empty() {
            return Err(xml_err(0, "no root element"));
        }
        skip_ws(&mut i);
        if i != b.len() {
            return Err(xml_err(i, "data after the root element"));
        }
        Ok(Document { nodes })
    }

    /// Number of elements.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn root(&self) -> Node<'_, 'a> {
        Node { doc: self, index: 0 }
    }

    /// All elements in document order.
    pub fn iter(&self) -> impl Iterator<Item = Node<'_, 'a>> {
        (0..self.nodes.len()).map(move |index| Node { doc: self, index })
    }
}

/// Iterates `key="value"` pairs of an attribute string.
fn attr_pairs(mut s: &str) -> impl Iterator<Item = Result<(&str, &str), &'static str>> {
    std::iter::from_fn(move || {
        s = s.trim_start();
        if s.is_empty() {
            return None;
        }
        let Some(eq) = s.find('=') else { return Some(Err("attribute without '='")) };
        let key = s[..eq].trim();
        let rest = s[eq + 1..].trim_start();
        let Some(rest) = rest.strip_prefix('"') else { return Some(Err("unquoted attribute")) };
        let Some(close) = rest.find('"') else { return Some(Err("unterminated attribute")) };
        let value = &rest[..close];
        s = &rest[close + 1..];
        Some(Ok((key, value)))
    })
}

fn check_attrs(s: &str) -> Result<(), &'static str> {
    for pair in attr_pairs(s) {
        let (_, v) = pair?;
        if v.contains('&') {
            unescape(v).ok_or("unknown entity")?;
        }
    }
    Ok(())
}

fn unescape(v: &str) -> Option<Cow<'_, str>> {
    if !v.contains('&') {
        return Some(Cow::Borrowed(v));
    }
    let mut out = String::with_capacity(v.len());
    let mut rest = v;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        let semi = rest[p..].find(';')? + p;
        let ent = &rest[p + 1..semi];
        let c = match ent {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ if ent.starts_with("#x") => char::from_u32(u32::from_str_radix(&ent[2..], 16).ok()?)?,
            _ if ent.starts_with('#') => char::from_u32(ent[1..].parse().ok()?)?,
            _ => return None,
        };
        out.push(c);
        rest = &rest[semi + 1..];
    }
    out.push_str(rest);
    Some(Cow::Owned(out))
}

/// One element of a [`Document`].
#[derive(Clone, Copy)]
pub struct Node<'d, 'a> {
    doc: &'d Document<'a>,
    index: usize,
}

impl std::fmt::Debug for Node<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<{} {}>", self.tag(), self.raw().attrs)
    }
}

impl<'d, 'a> Node<'d, 'a> {
    fn raw(&self) -> &Raw<'a> {
        &self.doc.nodes[self.index]
    }

    /// Position in document order.
    pub fn index(&self) -> usize {
        self.index
    }

    /// Element name: the value type (`class`, `u32`, `array`...).
    pub fn tag(&self) -> &'a str {
        self.raw().tag
    }

    /// An attribute, unescaped.
    pub fn attr(&self, key: &str) -> Option<Cow<'a, str>> {
        attr_pairs(self.raw().attrs)
            .filter_map(Result::ok)
            .find(|(k, _)| *k == key)
            .and_then(|(_, v)| unescape(v))
    }

    /// The `name` attribute: the engine's field name. Array elements and the
    /// root have none (or the root's object name).
    pub fn name(&self) -> Option<Cow<'a, str>> {
        self.attr("name")
    }

    /// The `type` attribute: the class name of a `class`, the element type
    /// of an `array`.
    pub fn ty(&self) -> Option<Cow<'a, str>> {
        self.attr("type")
    }

    /// The `value` attribute of a scalar.
    pub fn value(&self) -> Option<Cow<'a, str>> {
        self.attr("value")
    }

    /// The scalar value parsed as `T`.
    pub fn parse<T: FromStr>(&self) -> Option<T> {
        self.value()?.parse().ok()
    }

    /// The `count` attribute of an `array`.
    pub fn count(&self) -> Option<usize> {
        self.attr("count")?.parse().ok()
    }

    /// Direct children in order.
    pub fn children(&self) -> Children<'d, 'a> {
        Children { doc: self.doc, next: self.index + 1, end: self.raw().end as usize }
    }

    /// First direct child whose `name` is `name`.
    pub fn child(&self, name: &str) -> Option<Node<'d, 'a>> {
        self.children().find(|c| c.name().as_deref() == Some(name))
    }

    /// Follows a `/`-separated path of child names.
    pub fn path(&self, path: &str) -> Option<Node<'d, 'a>> {
        path.split('/').try_fold(*self, |n, part| n.child(part))
    }

    /// A scalar child parsed as `T`.
    pub fn get<T: FromStr>(&self, name: &str) -> Option<T> {
        self.child(name)?.parse()
    }

    /// The elements of an array child, parsed as `T`. `None` if the child is
    /// missing, is not an array, or an element does not parse.
    pub fn array<T: FromStr>(&self, name: &str) -> Option<Vec<T>> {
        let a = self.child(name)?;
        if a.tag() != "array" {
            return None;
        }
        a.children().map(|e| e.parse()).collect()
    }

    /// A `u8` array child read as a NUL-terminated string (names are stored
    /// this way, as UTF-8 bytes).
    pub fn byte_string(&self, name: &str) -> Option<String> {
        let bytes: Vec<u8> = self.array(name)?;
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        Some(String::from_utf8_lossy(&bytes[..end]).into_owned())
    }

    /// Descendants (not including self) in document order.
    pub fn descendants(&self) -> impl Iterator<Item = Node<'d, 'a>> + 'd {
        let doc = self.doc;
        (self.index + 1..self.raw().end as usize).map(move |index| Node { doc, index })
    }
}

/// Iterator over direct children.
pub struct Children<'d, 'a> {
    doc: &'d Document<'a>,
    next: usize,
    end: usize,
}

impl<'d, 'a> Iterator for Children<'d, 'a> {
    type Item = Node<'d, 'a>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.end {
            return None;
        }
        let n = Node { doc: self.doc, index: self.next };
        self.next = self.doc.nodes[self.next].end as usize;
        Some(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<class name="root" type="sSave::saveDataAllDA">
<u32 name="mCreateNo" value="21"/>
<array name="&amp;mChart[0][0]" type="s32" count="2">
<s32 value="-1"/>
<s32 value="5"/>
</array>
<class name="mEdit" type="cSAVE_DATA_EDIT">
<array name="(u8*)mNameStr" type="u8" count="4">
<u8 value="65"/>
<u8 value="98"/>
<u8 value="0"/>
<u8 value="7"/>
</array>
</class>
<vector3 name="mPos" x="1.0" y="2.5" z="-3"/>
<string name="mPath" value="AI\FSM\a&lt;b&gt;"/>
</class>
"#;

    #[test]
    fn parses_the_subset() {
        let d = Document::parse(SAMPLE).unwrap();
        let r = d.root();
        assert_eq!(r.ty().as_deref(), Some("sSave::saveDataAllDA"));
        assert_eq!(r.get::<u32>("mCreateNo"), Some(21));
        assert_eq!(r.array::<i32>("&mChart[0][0]"), Some(vec![-1, 5]));
        assert_eq!(r.child("&mChart[0][0]").unwrap().count(), Some(2));
        assert_eq!(r.path("mEdit").unwrap().byte_string("(u8*)mNameStr").as_deref(), Some("Ab"));
        let v = r.child("mPos").unwrap();
        assert_eq!(v.attr("y").as_deref(), Some("2.5"));
        assert_eq!(r.child("mPath").unwrap().value().as_deref(), Some("AI\\FSM\\a<b>"));
        assert_eq!(r.children().count(), 5);
        assert_eq!(r.descendants().count(), d.len() - 1);
    }

    #[test]
    fn rejects_malformed() {
        for bad in [
            "",
            "<a>",
            "<a></b>",
            "<a/><b/>",
            "<a>text</a>",
            "<a x=1/>",
            "<a x=\"&bogus;\"/>",
            "<a x=\"1/>",
            "</a>",
            "<a/> trailing",
        ] {
            assert!(Document::parse(bad).is_err(), "accepted {bad:?}");
        }
        // Every truncation of a valid document fails.
        for n in 0..SAMPLE.trim_end().len() {
            if SAMPLE.is_char_boundary(n) {
                assert!(Document::parse(&SAMPLE[..n]).is_err(), "accepted prefix {n}");
            }
        }
    }
}
