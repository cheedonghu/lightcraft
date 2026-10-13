//! JSON spans, with bounded buffers and bounded ancestor recovery. Strings and unknown
//! metadata are scanned rather than retained. No photo/record/root Value is built.
use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Span {
    pub start: u64,
    pub end: u64,
}
impl Span {
    pub fn len(self) -> Result<u64> {
        self.end.checked_sub(self.start).ok_or_else(|| error("invalid source span"))
    }
}
pub(crate) struct Json<'a> {
    pub source: &'a mut dyn SourceSnapshot,
    pub at: u64,
    cache: Vec<u8>,
    base: u64,
}
impl<'a> Json<'a> {
    pub fn new(source: &'a mut dyn SourceSnapshot, at: u64) -> Self {
        Self { source, at, cache: Vec::new(), base: at }
    }
    pub fn peek(&mut self) -> Result<Option<u8>> {
        if self.at >= self.source.identity().length {
            return Ok(None);
        }
        if self.at < self.base || self.at - self.base >= self.cache.len() as u64 {
            self.base = self.at;
            self.cache = self.source.read_range(self.at, (self.source.identity().length - self.at).min(32 * 1024) as usize).map_err(error)?;
        }
        Ok(self.cache.get(usize::try_from(self.at - self.base).map_err(error)?).copied())
    }
    fn take(&mut self) -> Result<u8> {
        let b = self.peek()?.ok_or_else(|| error("incomplete legacy JSON"))?;
        self.at = self.at.checked_add(1).ok_or_else(|| error("source cursor exhausted"))?;
        Ok(b)
    }
    pub fn space(&mut self) -> Result<()> {
        while self.peek()?.is_some_and(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n')) {
            self.take()?;
        }
        Ok(())
    }
    pub fn expect(&mut self, byte: u8) -> Result<()> {
        self.space()?;
        if self.take()? != byte {
            return Err(error("invalid legacy JSON punctuation"));
        }
        Ok(())
    }
    pub fn small(&mut self, span: Span) -> Result<Option<Value>> {
        if span.len()? > 256 * 1024 {
            return Ok(None);
        }
        let bytes = self.source.read_range(span.start, usize::try_from(span.len()?).map_err(error)?).map_err(error)?;
        serde_json::from_slice(&bytes).map(Some).map_err(error)
    }
    pub fn copy(&mut self, span: Span, writer: &mut dyn std::io::Write) -> Result<()> {
        let mut at = span.start;
        while at < span.end {
            let length = (span.end - at).min(32 * 1024) as usize;
            let bytes = self.source.read_range(at, length).map_err(error)?;
            writer.write_all(&bytes).map_err(error)?;
            at = at.checked_add(length as u64).ok_or_else(|| error("source span exhausted"))?;
        }
        Ok(())
    }
    pub fn string(&mut self) -> Result<Span> {
        self.space()?;
        let start = self.at;
        if self.take()? != b'"' {
            return Err(error("legacy JSON string expected"));
        }
        loop {
            match self.take()? {
                b'"' => return Ok(Span { start, end: self.at }),
                b'\\' => match self.take()? {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                    b'u' => {
                        let code = self.hex()?;
                        if (0xd800..=0xdbff).contains(&code) {
                            if self.take()? != b'\\' || self.take()? != b'u' {
                                return Err(error("invalid Unicode surrogate"));
                            }
                            let low = self.hex()?;
                            if !(0xdc00..=0xdfff).contains(&low) {
                                return Err(error("invalid Unicode surrogate"));
                            }
                        } else if (0xdc00..=0xdfff).contains(&code) {
                            return Err(error("invalid Unicode surrogate"));
                        }
                    }
                    _ => return Err(error("invalid legacy JSON escape")),
                },
                b if b < 0x20 => return Err(error("control byte in legacy JSON string")),
                b if b >= 0x80 => {
                    let length = if (0xc2..=0xdf).contains(&b) {
                        2
                    } else if (0xe0..=0xef).contains(&b) {
                        3
                    } else if (0xf0..=0xf4).contains(&b) {
                        4
                    } else {
                        return Err(error("invalid legacy UTF-8"));
                    };
                    let mut scalar = vec![b];
                    for _ in 1..length {
                        scalar.push(self.take()?);
                    }
                    std::str::from_utf8(&scalar).map_err(error)?;
                }
                _ => {}
            }
        }
    }
    fn hex(&mut self) -> Result<u16> {
        let mut value = 0u16;
        for _ in 0..4 {
            let b = self.take()?;
            let digit = (b as char).to_digit(16).ok_or_else(|| error("invalid Unicode escape"))?;
            value = value.checked_mul(16).and_then(|v| v.checked_add(digit as u16)).ok_or_else(|| error("Unicode escape overflow"))?;
        }
        Ok(value)
    }
    pub fn name(&mut self) -> Result<(Span, Option<String>)> {
        let span = self.string()?;
        let name = if span.len()? > 256 { None } else { self.small(span)?.and_then(|v| v.as_str().map(str::to_owned)) };
        Ok((span, name))
    }
    pub fn value(&mut self) -> Result<Span> {
        self.space()?;
        let start = self.at;
        // Only the nearest128 ancestors are retained. Exceptional deeper JSON
        // recovers ancestors by rereading the already validated source prefix.
        // Depth changes CPU work, never a validity/retention quota or stack size.
        let mut frames = Vec::<(u64, u8)>::with_capacity(128);
        let mut depth = 0u64;
        let mut first = false;
        let mut need_value = true;
        loop {
            if need_value {
                self.space()?;
                match self.peek()?.ok_or_else(|| error("legacy JSON value missing"))? {
                    b'{' | b'[' => {
                        let at = self.at;
                        let kind = self.take()?;
                        depth = depth.checked_add(1).ok_or_else(|| error("JSON depth exhausted"))?;
                        if frames.len() == 128 {
                            frames.remove(0);
                        }
                        frames.push((at, kind));
                        first = true;
                        need_value = false;
                    }
                    b'"' => {
                        self.string()?;
                        need_value = false;
                        first = false;
                    }
                    b't' => {
                        self.literal(b"true")?;
                        need_value = false;
                        first = false;
                    }
                    b'f' => {
                        self.literal(b"false")?;
                        need_value = false;
                        first = false;
                    }
                    b'n' => {
                        self.literal(b"null")?;
                        need_value = false;
                        first = false;
                    }
                    b'-' | b'0'..=b'9' => {
                        self.number()?;
                        need_value = false;
                        first = false;
                    }
                    _ => return Err(error("invalid legacy JSON value")),
                }
                if depth == 0 {
                    return Ok(Span { start, end: self.at });
                }
                if need_value {
                    continue;
                }
            }
            let (opened, kind) = frames.last().copied().ok_or_else(|| error("JSON ancestor missing"))?;
            self.space()?;
            let close = if kind == b'{' { b'}' } else { b']' };
            if self.peek()? == Some(close) {
                self.take()?;
                frames.pop();
                depth -= 1;
                if depth == 0 {
                    return Ok(Span { start, end: self.at });
                }
                if frames.is_empty() {
                    frames = self.ancestors(start, opened, depth)?;
                }
                first = false;
                continue;
            }
            if !first {
                self.expect(b',')?;
            }
            if kind == b'{' {
                self.string()?;
                self.expect(b':')?;
            }
            first = false;
            need_value = true;
        }
    }
    fn ancestors(&mut self, start: u64, until: u64, target: u64) -> Result<Vec<(u64, u8)>> {
        let low = target.saturating_sub(128);
        let mut frames = vec![(0, 0); usize::try_from(target - low).map_err(error)?];
        let mut scan = Json::new(self.source, start);
        let mut depth = 0u64;
        while scan.at < until {
            match scan.peek()?.ok_or_else(|| error("JSON ancestor prefix missing"))? {
                b'"' => {
                    scan.string()?;
                }
                b'{' | b'[' => {
                    let at = scan.at;
                    let kind = scan.take()?;
                    depth = depth.checked_add(1).ok_or_else(|| error("JSON depth exhausted"))?;
                    if depth > low && depth <= target {
                        let slot =
                            frames.get_mut(usize::try_from(depth - low - 1).map_err(error)?).ok_or_else(|| error("JSON ancestor slot missing"))?;
                        *slot = (at, kind);
                    }
                }
                b'}' | b']' => {
                    scan.take()?;
                    depth = depth.checked_sub(1).ok_or_else(|| error("JSON ancestor prefix invalid"))?;
                }
                _ => {
                    scan.take()?;
                }
            }
        }
        if depth != target || frames.iter().any(|(_, kind)| !matches!(kind, b'{' | b'[')) {
            return Err(error("JSON ancestor recovery mismatch"));
        }
        Ok(frames)
    }
    fn literal(&mut self, bytes: &[u8]) -> Result<()> {
        for byte in bytes {
            if self.take()? != *byte {
                return Err(error("invalid legacy literal"));
            }
        }
        Ok(())
    }
    fn digits(&mut self) -> Result<()> {
        let mut any = false;
        while self.peek()?.is_some_and(|b| b.is_ascii_digit()) {
            self.take()?;
            any = true;
        }
        if !any {
            return Err(error("invalid legacy number"));
        }
        Ok(())
    }
    fn number(&mut self) -> Result<()> {
        if self.peek()? == Some(b'-') {
            self.take()?;
        }
        if self.peek()? == Some(b'0') {
            self.take()?;
        } else {
            if !self.peek()?.is_some_and(|b| matches!(b, b'1'..=b'9')) {
                return Err(error("invalid legacy number"));
            }
            self.digits()?;
        }
        if self.peek()? == Some(b'.') {
            self.take()?;
            self.digits()?;
        }
        if self.peek()?.is_some_and(|b| matches!(b, b'e' | b'E')) {
            self.take()?;
            if self.peek()?.is_some_and(|b| matches!(b, b'+' | b'-')) {
                self.take()?;
            }
            self.digits()?;
        }
        Ok(())
    }
    /// The next member of an already-open object, retaining only its name/span.
    pub fn member(&mut self, first: &mut bool) -> Result<Option<(Span, Option<String>, Span)>> {
        self.space()?;
        if self.peek()? == Some(b'}') {
            self.take()?;
            return Ok(None);
        }
        if !*first {
            self.expect(b',')?;
        }
        *first = false;
        let (key, name) = self.name()?;
        self.expect(b':')?;
        let value = self.value()?;
        Ok(Some((key, name, value)))
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub(super) struct Root {
    pub photos: Option<Span>,
    pub v2: bool,
}
pub(super) fn root(source: &mut dyn SourceSnapshot) -> Result<Root> {
    let mut json = Json::new(source, 0);
    json.expect(b'{')?;
    let mut first = true;
    let (mut format, mut version, mut layout, mut photos) = (None, None, None, None);
    while let Some((_, name, value)) = json.member(&mut first)? {
        match name.as_deref() {
            Some("format") => {
                if format.is_some() {
                    return Err(error("duplicate archive format"));
                }
                format = Some(json.small(value)?.ok_or_else(|| error("unsupported archive format"))?);
            }
            Some("version") => {
                if version.is_some() {
                    return Err(error("duplicate archive version"));
                }
                version = Some(json.small(value)?.ok_or_else(|| error("unsupported archive version"))?);
            }
            Some("layout") => {
                if layout.is_some() {
                    return Err(error("duplicate archive layout"));
                }
                layout = Some(value);
            }
            Some("photos") => {
                if photos.is_some() {
                    return Err(error("duplicate archive photos"));
                }
                photos = Some(value);
            }
            _ => {}
        }
    }
    json.space()?;
    if json.peek()?.is_some() {
        return Err(error("trailing legacy data; source protected"));
    }
    if format != Some(json!("lightcraft-companion")) {
        return Err(error("unsupported archive format; source protected"));
    }
    if version == Some(json!(2)) {
        let layout = layout.map(|span| json.small(span)).transpose()?.flatten();
        if layout == Some(json!("photo-head-v2")) {
            return Ok(Root { photos: None, v2: true });
        }
    }
    if version != Some(json!(1)) {
        return Err(error("unsupported archive version; source protected"));
    }
    let photos = photos.ok_or_else(|| error("legacy photos missing"))?;
    let mut check = Json::new(json.source, photos.start);
    check.expect(b'{')?;
    Ok(Root { photos: Some(photos), v2: false })
}
