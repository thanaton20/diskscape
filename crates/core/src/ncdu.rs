//! Import and export of ncdu's JSON export format (major version 1).
//!
//! ```text
//! [1, 2, {"progname": …, "progver": …, "timestamp": …}, <item>]
//! <item> = {object}                      a file or anything that is not a listed directory
//!        | [{object}, <item>, <item>…]   a directory followed by its contents
//! ```
//!
//! The parser is a hand-written pull parser rather than a serde visitor for
//! two reasons: directory nesting must not be limited by the call stack, and
//! ncdu writes file names as raw bytes that are not always valid UTF-8.

use crate::tree::{Entry, NodeId, NodeKind, Tree};
use std::ffi::OsString;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::PathBuf;

#[derive(Debug)]
pub enum NcduError {
    Io(io::Error),
    /// Not well-formed JSON.
    Json(String),
    /// Well-formed, but a major format version this program does not know.
    UnsupportedVersion(u64),
    /// Well-formed JSON that is not an ncdu export.
    Malformed(&'static str),
}

impl std::fmt::Display for NcduError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NcduError::Io(e) => write!(f, "cannot read the file: {e}"),
            NcduError::Json(msg) => write!(f, "not valid JSON: {msg}"),
            NcduError::UnsupportedVersion(v) => write!(f, "unsupported ncdu format version {v}"),
            NcduError::Malformed(msg) => write!(f, "not an ncdu export: {msg}"),
        }
    }
}

impl std::error::Error for NcduError {}

impl From<io::Error> for NcduError {
    fn from(e: io::Error) -> Self {
        NcduError::Io(e)
    }
}

// ---------------------------------------------------------------- export

/// Writes `tree` in ncdu's JSON format, readable by `ncdu -f`.
pub fn export<W: Write>(tree: &Tree, w: W) -> io::Result<()> {
    enum Step {
        Enter(NodeId),
        CloseDir,
    }

    let mut w = BufWriter::new(w);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    write!(
        w,
        "[1,2,{{\"progname\":\"diskscape\",\"progver\":\"{}\",\"timestamp\":{}}}",
        env!("CARGO_PKG_VERSION"),
        timestamp
    )?;

    let mut stack = vec![Step::Enter(tree.root())];
    while let Some(step) = stack.pop() {
        let id = match step {
            Step::CloseDir => {
                w.write_all(b"]")?;
                continue;
            }
            Step::Enter(id) => id,
        };
        let Some(node) = tree.node(id) else { continue };
        w.write_all(b",\n")?;

        let listed_dir = node.kind == NodeKind::Dir && !node.flags.other_fs;
        if listed_dir {
            w.write_all(b"[")?;
        }
        w.write_all(b"{\"name\":")?;
        let name = match node.parent {
            Some(_) => node.name.to_string_lossy(),
            None => tree.path(id).to_string_lossy().into_owned().into(),
        };
        write_json_string(&mut w, &name)?;
        if node.own_asize > 0 {
            write!(w, ",\"asize\":{}", node.own_asize)?;
        }
        if node.own_dsize > 0 {
            write!(w, ",\"dsize\":{}", node.own_dsize)?;
        }
        if node.kind == NodeKind::Dir {
            let parent_dev = node.parent.and_then(|p| tree.node(p)).map(|p| p.dev);
            if parent_dev != Some(node.dev) {
                write!(w, ",\"dev\":{}", node.dev)?;
            }
        } else if node.nlink > 1 {
            write!(w, ",\"ino\":{},\"hlnkc\":true,\"nlink\":{}", node.ino, node.nlink)?;
        }
        if node.flags.read_error {
            w.write_all(b",\"read_error\":true")?;
        }
        if node.flags.other_fs {
            w.write_all(b",\"excluded\":\"otherfs\"")?;
        }
        if matches!(node.kind, NodeKind::Symlink | NodeKind::Other) {
            w.write_all(b",\"notreg\":true")?;
        }
        w.write_all(b"}")?;

        if listed_dir {
            stack.push(Step::CloseDir);
            stack.extend(node.children.iter().rev().map(|c| Step::Enter(*c)));
        }
    }
    w.write_all(b"]\n")?;
    w.flush()
}

fn write_json_string<W: Write>(w: &mut W, s: &str) -> io::Result<()> {
    w.write_all(b"\"")?;
    for c in s.chars() {
        match c {
            '"' => w.write_all(b"\\\"")?,
            '\\' => w.write_all(b"\\\\")?,
            '\n' => w.write_all(b"\\n")?,
            '\r' => w.write_all(b"\\r")?,
            '\t' => w.write_all(b"\\t")?,
            c if (c as u32) < 0x20 || c == '\u{7f}' => write!(w, "\\u{:04x}", c as u32)?,
            c => write!(w, "{c}")?,
        }
    }
    w.write_all(b"\"")
}

// ---------------------------------------------------------------- import

/// Reads an ncdu export. The resulting tree is read-only: it may describe
/// another machine or another moment, so nothing in it can be deleted.
pub fn import<R: Read>(r: R) -> Result<Tree, NcduError> {
    let mut lex = Lexer::new(r);
    lex.expect(Tok::ArrBegin, "an ncdu export starts with '['")?;
    let major = match lex.next_token()? {
        Tok::Num(Some(n)) => n,
        _ => return Err(NcduError::Malformed("missing format version")),
    };
    if major != 1 {
        return Err(NcduError::UnsupportedVersion(major));
    }
    lex.expect(Tok::Comma, "missing minor version")?;
    if !matches!(lex.next_token()?, Tok::Num(_)) {
        return Err(NcduError::Malformed("missing minor version"));
    }
    lex.expect(Tok::Comma, "missing metadata block")?;
    let metadata = lex.next_token()?;
    lex.skip_value(metadata)?;
    lex.expect(Tok::Comma, "missing root item")?;

    let mut tree = read_items(&mut lex)?;
    lex.expect(Tok::ArrEnd, "missing closing ']'")?;
    tree.finalize();
    Ok(tree)
}

/// Reads the root item and everything nested in it, without recursion.
fn read_items<R: Read>(lex: &mut Lexer<R>) -> Result<Tree, NcduError> {
    let mut tree: Option<Tree> = None;
    // Directories currently open, each with the device id its children inherit.
    let mut open: Vec<(NodeId, u64)> = Vec::new();

    loop {
        let is_dir = match lex.next_token()? {
            Tok::ObjBegin => false,
            Tok::ArrBegin => {
                lex.expect(Tok::ObjBegin, "a directory must start with its own entry")?;
                true
            }
            _ => return Err(NcduError::Malformed("expected a file or directory entry")),
        };
        let info = read_object(lex)?;
        let inherited_dev = open.last().map_or(0, |(_, dev)| *dev);
        let dev = info.dev.unwrap_or(inherited_dev);
        let entry = info.into_entry(is_dir, dev);

        let id = match (&mut tree, open.last()) {
            (Some(tree), Some((parent, _))) => tree.add_child(*parent, entry),
            (None, None) => {
                let root_path = PathBuf::from(entry.name.clone());
                let new_tree = tree.insert(Tree::new(root_path, entry, true));
                new_tree.root()
            }
            _ => return Err(NcduError::Malformed("more than one root item")),
        };
        if is_dir {
            open.push((id, dev));
        }

        // Close finished directories until a sibling follows or the root is done.
        loop {
            if open.is_empty() {
                return tree.ok_or(NcduError::Malformed("missing root item"));
            }
            match lex.next_token()? {
                Tok::Comma => break,
                Tok::ArrEnd => {
                    open.pop();
                }
                _ => return Err(NcduError::Malformed("expected ',' or ']' after an entry")),
            }
        }
    }
}

#[derive(Default)]
struct Info {
    name: Option<Vec<u8>>,
    asize: u64,
    dsize: u64,
    dev: Option<u64>,
    ino: u64,
    hlnkc: bool,
    nlink: Option<u64>,
    read_error: bool,
    excluded: Option<Vec<u8>>,
    notreg: bool,
}

impl Info {
    fn into_entry(self, is_dir: bool, dev: u64) -> Entry {
        // ncdu writes excluded directories as plain objects, not as lists.
        let excluded_dir = matches!(
            self.excluded.as_deref(),
            Some(b"otherfs" | b"othfs" | b"kernfs" | b"frmlink")
        );
        let kind = if is_dir || excluded_dir {
            NodeKind::Dir
        } else if self.notreg || self.excluded.is_some() {
            NodeKind::Other
        } else {
            NodeKind::File
        };
        let mut e = Entry::new(bytes_to_os_string(self.name.unwrap_or_default()), kind, self.asize, self.dsize);
        e.dev = dev;
        e.ino = self.ino;
        // ncdu 1.x marks hard links with `hlnkc` only, without a link count.
        e.nlink = if self.hlnkc { self.nlink.unwrap_or(2).max(2) } else { 1 };
        e.read_error = self.read_error;
        e.other_fs = excluded_dir && !is_dir;
        e
    }
}

#[cfg(unix)]
fn bytes_to_os_string(bytes: Vec<u8>) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(bytes)
}

#[cfg(not(unix))]
fn bytes_to_os_string(bytes: Vec<u8>) -> OsString {
    OsString::from(String::from_utf8_lossy(&bytes).into_owned())
}

/// Reads the members of an object whose '{' has already been consumed.
fn read_object<R: Read>(lex: &mut Lexer<R>) -> Result<Info, NcduError> {
    let mut info = Info::default();
    loop {
        let key = match lex.next_token()? {
            Tok::ObjEnd => break,
            Tok::Str(key) => key,
            _ => return Err(lex.syntax("expected a field name")),
        };
        lex.expect(Tok::Colon, "expected ':' after a field name")?;
        let value = lex.next_token()?;
        match (key.as_slice(), value) {
            (b"name", Tok::Str(s)) => info.name = Some(s),
            (b"asize", Tok::Num(Some(n))) => info.asize = n,
            (b"dsize", Tok::Num(Some(n))) => info.dsize = n,
            (b"dev", Tok::Num(Some(n))) => info.dev = Some(n),
            (b"ino", Tok::Num(Some(n))) => info.ino = n,
            (b"nlink", Tok::Num(Some(n))) => info.nlink = Some(n),
            (b"hlnkc", Tok::Bool(b)) => info.hlnkc = b,
            (b"read_error", Tok::Bool(b)) => info.read_error = b,
            (b"notreg", Tok::Bool(b)) => info.notreg = b,
            (b"excluded", Tok::Str(s)) => info.excluded = Some(s),
            (
                b"name" | b"asize" | b"dsize" | b"dev" | b"ino" | b"nlink" | b"hlnkc"
                | b"read_error" | b"notreg" | b"excluded",
                _,
            ) => return Err(NcduError::Malformed("a known field has the wrong type")),
            (_, other) => lex.skip_value(other)?,
        }
        match lex.next_token()? {
            Tok::Comma => {}
            Tok::ObjEnd => break,
            _ => return Err(lex.syntax("expected ',' or '}'")),
        }
    }
    if info.name.is_none() {
        return Err(NcduError::Malformed("entry without a name"));
    }
    Ok(info)
}

#[derive(Debug, PartialEq)]
enum Tok {
    ArrBegin,
    ArrEnd,
    ObjBegin,
    ObjEnd,
    Colon,
    Comma,
    /// Raw bytes: escapes are decoded, bytes that are not valid UTF-8 are kept.
    Str(Vec<u8>),
    /// `Some` for a non-negative integer that fits in `u64`, `None` for any other number.
    Num(Option<u64>),
    Bool(bool),
    Null,
    Eof,
}

struct Lexer<R: Read> {
    bytes: io::Bytes<BufReader<R>>,
    peeked: Option<u8>,
    offset: u64,
}

impl<R: Read> Lexer<R> {
    fn new(r: R) -> Self {
        Lexer { bytes: BufReader::new(r).bytes(), peeked: None, offset: 0 }
    }

    fn syntax(&self, what: &str) -> NcduError {
        NcduError::Json(format!("{what} near byte {}", self.offset))
    }

    fn bump(&mut self) -> Result<Option<u8>, NcduError> {
        if let Some(b) = self.peeked.take() {
            return Ok(Some(b));
        }
        match self.bytes.next() {
            None => Ok(None),
            Some(Ok(b)) => {
                self.offset += 1;
                Ok(Some(b))
            }
            Some(Err(e)) => Err(NcduError::Io(e)),
        }
    }

    fn peek(&mut self) -> Result<Option<u8>, NcduError> {
        if self.peeked.is_none() {
            self.peeked = self.bump()?;
        }
        Ok(self.peeked)
    }

    fn expect(&mut self, want: Tok, why: &'static str) -> Result<(), NcduError> {
        if self.next_token()? == want {
            Ok(())
        } else {
            Err(NcduError::Malformed(why))
        }
    }

    fn next_token(&mut self) -> Result<Tok, NcduError> {
        let first = loop {
            match self.bump()? {
                None => return Ok(Tok::Eof),
                Some(b' ' | b'\t' | b'\n' | b'\r') => continue,
                Some(b) => break b,
            }
        };
        match first {
            b'[' => Ok(Tok::ArrBegin),
            b']' => Ok(Tok::ArrEnd),
            b'{' => Ok(Tok::ObjBegin),
            b'}' => Ok(Tok::ObjEnd),
            b':' => Ok(Tok::Colon),
            b',' => Ok(Tok::Comma),
            b'"' => self.read_string().map(Tok::Str),
            b'-' | b'0'..=b'9' => {
                let mut text = vec![first];
                while let Some(b @ (b'+' | b'-' | b'.' | b'e' | b'E' | b'0'..=b'9')) = self.peek()? {
                    text.push(b);
                    self.peeked = None;
                }
                let text = String::from_utf8_lossy(&text);
                if text.parse::<f64>().is_err() {
                    return Err(self.syntax("malformed number"));
                }
                Ok(Tok::Num(text.parse::<u64>().ok()))
            }
            b'a'..=b'z' => {
                let mut word = vec![first];
                while let Some(b @ b'a'..=b'z') = self.peek()? {
                    word.push(b);
                    self.peeked = None;
                }
                match word.as_slice() {
                    b"true" => Ok(Tok::Bool(true)),
                    b"false" => Ok(Tok::Bool(false)),
                    b"null" => Ok(Tok::Null),
                    _ => Err(self.syntax("unknown literal")),
                }
            }
            _ => Err(self.syntax("unexpected character")),
        }
    }

    /// Reads a string whose opening quote has already been consumed.
    fn read_string(&mut self) -> Result<Vec<u8>, NcduError> {
        fn push_char(out: &mut Vec<u8>, c: char) {
            out.extend_from_slice(c.encode_utf8(&mut [0u8; 4]).as_bytes());
        }

        let mut out = Vec::new();
        // First half of a 😀-style surrogate pair, waiting for its second half.
        let mut high: Option<u16> = None;
        loop {
            let Some(b) = self.bump()? else {
                return Err(self.syntax("unterminated string"));
            };
            let unit = match b {
                b'"' | b'\\' => {
                    if b == b'"' {
                        if high.is_some() {
                            push_char(&mut out, char::REPLACEMENT_CHARACTER);
                        }
                        return Ok(out);
                    }
                    let Some(esc) = self.bump()? else {
                        return Err(self.syntax("unterminated string"));
                    };
                    match esc {
                        b'u' => Some(self.read_hex4()?),
                        other => {
                            if high.take().is_some() {
                                push_char(&mut out, char::REPLACEMENT_CHARACTER);
                            }
                            out.push(match other {
                                b'"' => b'"',
                                b'\\' => b'\\',
                                b'/' => b'/',
                                b'b' => 0x08,
                                b'f' => 0x0c,
                                b'n' => b'\n',
                                b'r' => b'\r',
                                b't' => b'\t',
                                _ => return Err(self.syntax("unknown escape")),
                            });
                            None
                        }
                    }
                }
                raw => {
                    if high.take().is_some() {
                        push_char(&mut out, char::REPLACEMENT_CHARACTER);
                    }
                    out.push(raw);
                    None
                }
            };
            let Some(unit) = unit else { continue };
            match (high.take(), unit) {
                (Some(h), 0xDC00..=0xDFFF) => {
                    let code = 0x10000 + ((u32::from(h) - 0xD800) << 10) + (u32::from(unit) - 0xDC00);
                    push_char(&mut out, char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER));
                }
                (pending, unit) => {
                    if pending.is_some() {
                        push_char(&mut out, char::REPLACEMENT_CHARACTER);
                    }
                    match unit {
                        0xD800..=0xDBFF => high = Some(unit),
                        _ => push_char(
                            &mut out,
                            char::from_u32(u32::from(unit)).unwrap_or(char::REPLACEMENT_CHARACTER),
                        ),
                    }
                }
            }
        }
    }

    fn read_hex4(&mut self) -> Result<u16, NcduError> {
        let mut unit = 0u16;
        for _ in 0..4 {
            let digit = match self.bump()? {
                Some(b @ b'0'..=b'9') => b - b'0',
                Some(b @ b'a'..=b'f') => b - b'a' + 10,
                Some(b @ b'A'..=b'F') => b - b'A' + 10,
                _ => return Err(self.syntax("malformed \\u escape")),
            };
            unit = (unit << 4) | u16::from(digit);
        }
        Ok(unit)
    }

    /// Skips one value of any shape, given its first token. Iterative, so
    /// arbitrarily nested unknown fields cannot overflow the stack.
    fn skip_value(&mut self, first: Tok) -> Result<(), NcduError> {
        let mut depth = 0usize;
        let mut tok = first;
        loop {
            match tok {
                Tok::ArrBegin | Tok::ObjBegin => depth += 1,
                Tok::ArrEnd | Tok::ObjEnd => {
                    depth = depth.checked_sub(1).ok_or_else(|| self.syntax("unbalanced bracket"))?;
                }
                Tok::Eof => return Err(self.syntax("unexpected end of file")),
                Tok::Colon | Tok::Comma if depth == 0 => return Err(self.syntax("expected a value")),
                _ => {}
            }
            if depth == 0 {
                return Ok(());
            }
            tok = self.next_token()?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{Entry, NodeKind, Tree};
    use std::path::PathBuf;

    const FIXTURE: &str = include_str!("../tests/fixtures/ncdu-2.9.2.json");

    type Row = (String, NodeKind, u64, u64, bool, bool, bool, u64, u64);

    /// Everything that must survive a round trip, in name-ordered pre-order.
    /// Symbolic links come back as `Other`: the format only records "not a regular file".
    fn snapshot(t: &Tree) -> Vec<Row> {
        let mut out = Vec::new();
        let mut stack = vec![t.root()];
        while let Some(id) = stack.pop() {
            let n = t.node(id).unwrap();
            let kind = if n.kind == NodeKind::Symlink { NodeKind::Other } else { n.kind };
            out.push((
                t.path(id).to_string_lossy().into_owned(),
                kind,
                n.own_asize,
                n.own_dsize,
                n.flags.read_error,
                n.flags.other_fs,
                n.flags.hardlink_dup,
                n.dsize,
                n.items,
            ));
            stack.extend(n.children.iter().rev().copied());
        }
        out
    }

    fn import_str(s: &str) -> Result<Tree, NcduError> {
        import(s.as_bytes())
    }

    fn export_string(t: &Tree) -> String {
        let mut buf = Vec::new();
        export(t, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn rich_tree() -> Tree {
        let mut t = Tree::new(PathBuf::from("/r"), Entry::new("/r", NodeKind::Dir, 128, 0), false);
        let root = t.root();
        t.add_child(root, Entry::new("plain.txt", NodeKind::File, 5, 4096));
        t.add_child(root, Entry::new("zero", NodeKind::File, 0, 0));
        t.add_child(root, Entry::new("we\"ird\n\\name\tф", NodeKind::File, 1, 512));
        t.add_child(root, Entry::new("link", NodeKind::Symlink, 9, 0));
        t.add_child(root, Entry::new("empty", NodeKind::Dir, 64, 0));
        let mut locked = Entry::new("locked", NodeKind::Dir, 64, 0);
        locked.read_error = true;
        t.add_child(root, locked);
        let mut mount = Entry::new("mnt", NodeKind::Dir, 0, 0);
        mount.other_fs = true;
        t.add_child(root, mount);
        let sub = t.add_child(root, Entry::new("sub", NodeKind::Dir, 96, 0));
        for name in ["h1", "h2"] {
            let mut e = Entry::new(name, NodeKind::File, 7, 8192);
            e.ino = 42;
            e.nlink = 2;
            t.add_child(if name == "h1" { root } else { sub }, e);
        }
        t.finalize();
        t
    }

    #[test]
    fn import_fixture_from_real_ncdu() {
        let t = import_str(FIXTURE).unwrap();
        assert!(t.read_only());
        assert_eq!(t.path(t.root()), PathBuf::from("/sample"));
        let root = t.node(t.root()).unwrap();
        assert_eq!(root.items, 7);
        assert_eq!(root.dsize, 4096 * 3);
        assert_eq!(root.asize, 128 + 192 + 96 + 2 + 1 + 5 + 5);
        assert_eq!(root.dev, 16777233);
    }

    #[test]
    fn export_then_import_gives_same_tree() {
        let t = rich_tree();
        let back = import_str(&export_string(&t)).unwrap();
        assert_eq!(snapshot(&back), snapshot(&t));
        assert!(back.read_only());
    }

    #[test]
    fn export_of_import_is_stable() {
        let once = export_string(&import_str(FIXTURE).unwrap());
        let twice = export_string(&import_str(&once).unwrap());
        let strip = |s: &str| s.lines().skip(1).collect::<Vec<_>>().join("\n");
        assert_eq!(strip(&once), strip(&twice));
    }

    #[test]
    fn export_writes_version_header() {
        let out = export_string(&rich_tree());
        assert!(out.starts_with("[1,2,{\"progname\":\"diskscape\""));
        assert!(out.trim_end().ends_with("]]"));
    }

    #[test]
    fn name_with_quote_and_newline_round_trips() {
        let t = rich_tree();
        let out = export_string(&t);
        assert!(out.contains(r#"we\"ird\n\\name\tф"#));
        let back = import_str(&out).unwrap();
        // compare the name, not the joined path: the separator differs by platform
        let names: Vec<_> = back
            .node(back.root())
            .unwrap()
            .children
            .iter()
            .map(|c| back.node(*c).unwrap().name.clone())
            .collect();
        assert!(names.iter().any(|n| n == "we\"ird\n\\name\tф"));
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let t = import_str(
            r#"[1,9,{"progname":"x","future":[1,{"a":null}]},
               [{"name":"/r","uid":501,"extra":{"x":[1,2,{"y":null}],"z":"s"}},
                {"name":"a","asize":1,"mtime":1.5e3,"neg":-4,"flag":false,"dsize":512}]]"#,
        )
        .unwrap();
        assert_eq!(t.node(t.root()).unwrap().dsize, 512);
        assert_eq!(t.node(t.root()).unwrap().items, 1);
    }

    #[test]
    fn json_escapes_are_decoded() {
        let t = import_str(r#"[1,2,{},[{"name":"/r"},{"name":"ф😀\/A"}]]"#).unwrap();
        let child = t.node(t.root()).unwrap().children[0];
        assert_eq!(t.node(child).unwrap().name, "ф😀/A");
    }

    #[cfg(unix)]
    #[test]
    fn raw_non_utf8_name_bytes_are_kept() {
        use std::os::unix::ffi::OsStrExt;
        let mut data = br#"[1,2,{},[{"name":"/r"},{"name":"bad"#.to_vec();
        data.push(0xff);
        data.extend_from_slice(br#"name"}]]"#);
        let t = import(&data[..]).unwrap();
        let child = t.node(t.root()).unwrap().children[0];
        assert_eq!(t.node(child).unwrap().name.as_bytes(), b"bad\xffname");
        // and exporting it must still produce valid UTF-8
        export_string(&t);
    }

    #[test]
    fn hardlink_without_nlink_is_still_deduplicated() {
        let t = import_str(
            r#"[1,0,{},[{"name":"/r","dev":3},
                {"name":"a","dsize":4096,"ino":9,"hlnkc":true},
                {"name":"b","dsize":4096,"ino":9,"hlnkc":true}]]"#,
        )
        .unwrap();
        assert_eq!(t.node(t.root()).unwrap().dsize, 4096);
    }

    #[test]
    fn excluded_otherfs_is_a_directory_on_another_filesystem() {
        let t = import_str(r#"[1,2,{},[{"name":"/r"},{"name":"mnt","excluded":"otherfs"}]]"#).unwrap();
        let mnt = t.node(t.node(t.root()).unwrap().children[0]).unwrap();
        assert_eq!(mnt.kind, NodeKind::Dir);
        assert!(mnt.flags.other_fs);
    }

    #[test]
    fn root_may_be_a_single_file() {
        let t = import_str(r#"[1,2,{},{"name":"/r/a.txt","asize":5,"dsize":4096}]"#).unwrap();
        let root = t.node(t.root()).unwrap();
        assert_eq!(root.kind, NodeKind::File);
        assert_eq!(root.dsize, 4096);
        assert_eq!(import_str(&export_string(&t)).unwrap().node(0).unwrap().dsize, 4096);
    }

    #[test]
    fn depth_10000_does_not_overflow_the_stack() {
        let depth = 10_000;
        let mut s = String::from(r#"[1,2,{},[{"name":"/r"}"#);
        for _ in 0..depth {
            s.push_str(r#",[{"name":"d"}"#);
        }
        s.push_str(&"]".repeat(depth + 2));
        let t = import_str(&s).unwrap();
        assert_eq!(t.node(t.root()).unwrap().items, depth as u64);
        assert!(export_string(&t).len() > depth);
    }

    #[test]
    fn major_version_2_is_unsupported() {
        assert!(matches!(
            import_str(r#"[2,0,{},[{"name":"/r"}]]"#),
            Err(NcduError::UnsupportedVersion(2))
        ));
    }

    #[test]
    fn truncated_file_is_error_not_panic() {
        let full = FIXTURE.trim_end();
        for cut in 0..full.len() {
            if !full.is_char_boundary(cut) {
                continue;
            }
            assert!(import_str(&full[..cut]).is_err(), "prefix of {cut} bytes was accepted");
        }
    }

    #[test]
    fn garbage_is_error_not_panic() {
        let inputs: [&[u8]; 14] = [
            b"",
            b"\x00\xffnot json",
            b"[]",
            b"{}",
            b"[1]",
            b"[1,2]",
            b"[1,2,{}]",
            b"[1,2,{},[]]",
            b"[1,2,{},[{}]]",
            b"[1,2,{},[{\"asize\":1}]]",
            b"[1,2,{},[{\"name\":7}]]",
            b"[\"1\",2,{},[{\"name\":\"/r\"}]]",
            b"[1,2,{},[{\"name\":\"/r\"},,]]",
            b"[1,2,{},[{\"name\":\"/r\" \"x\":1}]]",
        ];
        for input in inputs {
            assert!(import(input).is_err(), "accepted {:?}", String::from_utf8_lossy(input));
        }
    }
}
