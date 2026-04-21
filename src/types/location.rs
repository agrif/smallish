pub type LocResult<'de, T, E> = Result<Located<'de, T>, Located<'de, E>>;

#[derive(Clone, Copy, Eq, serde::Deserialize)]
#[serde(rename = "__smallish_magic_located__")]
pub struct Located<'a, T> {
    pub source: Option<&'a [u8]>,
    pub line: usize,
    pub column: usize,
    pub offset: usize,
    pub value: T,
}

impl Located<'static, ()> {
    pub(crate) const SERDE_NAME: &'static str = "__smallish_magic_located__";

    pub const fn new() -> Self {
        Self {
            source: None,
            line: 1,
            column: 0,
            offset: 0,
            value: (),
        }
    }
}

impl<'de, T> Located<'de, T> {
    pub fn source_line(&self) -> Option<&'de str> {
        self.source_line_bytes()
            .and_then(|s| core::str::from_utf8(s).ok())
    }

    pub fn source_line_bytes(&self) -> Option<&'de [u8]> {
        let source = self.source?;
        let start = source
            .get(..self.offset)?
            .iter()
            .rposition(|c| *c == b'\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let end = source
            .get(self.offset..)?
            .iter()
            .position(|c| *c == b'\n')
            .map(|i| self.offset + i)
            .unwrap_or(source.len());
        Some(&source[start..end])
    }

    pub(crate) fn advance(&mut self, start: &'de [u8], end: &'de [u8]) {
        if start.len() < end.len() {
            return;
        }

        let new = &start[..start.len() - end.len()];
        let amt = new.len();
        let mut newlines = 0;
        let mut last_newline = None;
        for (i, _) in new.iter().enumerate().filter(|(_, c)| **c == b'\n') {
            newlines += 1;
            last_newline = Some(i);
        }

        if let Some(last_newline) = last_newline {
            self.line += newlines;
            self.column = amt - last_newline - 1;
        } else {
            self.column += amt;
        }
        self.offset += amt;
    }

    pub fn wrap<U>(&self, value: U) -> Located<'de, U> {
        Located {
            source: self.source,
            line: self.line,
            column: self.column,
            offset: self.offset,
            value,
        }
    }

    pub fn replace<U>(self, value: U) -> Located<'de, U> {
        self.map(|_| value)
    }

    pub fn split(self) -> (Located<'de, ()>, T) {
        (
            Located {
                source: self.source,
                line: self.line,
                column: self.column,
                offset: self.offset,
                value: (),
            },
            self.value,
        )
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Located<'de, U> {
        Located {
            source: self.source,
            line: self.line,
            column: self.column,
            offset: self.offset,
            value: f(self.value),
        }
    }

    pub fn with_source<'a>(self, source: Option<&'a [u8]>) -> Located<'a, T> {
        Located { source, ..self }
    }

    pub fn without_source(self) -> Located<'static, T> {
        Located {
            source: None,
            ..self
        }
    }
}

impl<'de, T, E> Located<'de, Result<T, E>> {
    pub fn from_result(result: LocResult<'de, T, E>) -> Self {
        match result {
            Ok(t) => t.map(Ok),
            Err(e) => e.map(Err),
        }
    }

    pub fn to_result(self) -> LocResult<'de, T, E> {
        let (loc, val) = self.split();
        match val {
            Ok(t) => Ok(loc.replace(t)),
            Err(e) => Err(loc.replace(e)),
        }
    }
}

impl<'de, T> Default for Located<'de, T>
where
    T: Default,
{
    fn default() -> Self {
        Self {
            source: None,
            line: 1,
            column: 0,
            offset: 0,
            value: Default::default(),
        }
    }
}

impl<'de, T> core::ops::Deref for Located<'de, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<'de, T> core::ops::DerefMut for Located<'de, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

impl<'de, T> core::cmp::PartialEq for Located<'de, T>
where
    T: core::cmp::PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        // ignore source
        self.line == other.line
            && self.column == other.column
            && self.offset == other.offset
            && self.value == other.value
    }
}

impl<'de, T> core::fmt::Debug for Located<'de, T>
where
    T: core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Located")
            // use dummy value for source to avoid spam
            .field("source", &self.source.map(|_| "..."))
            .field("line", &self.line)
            .field("column", &self.column)
            .field("offset", &self.offset)
            .field("value", &self.value)
            .finish()
    }
}

#[cfg(feature = "defmt")]
impl<'de, T> defmt::Format for Located<'de, T>
where
    T: defmt::Format,
{
    fn format(&self, f: defmt::Formatter) {
        if let Some(line) = self.source_line_bytes() {
            defmt::write!(
                f,
                "<at {0=usize}:{1=usize} ({3=[u8]:a})> {2}",
                self.line,
                self.column,
                self.value,
                line,
            )
        } else {
            defmt::write!(
                f,
                "<at {0=usize}:{1=usize}> {2}",
                self.line,
                self.column,
                self.value
            )
        }
    }
}

impl<'de, T> core::fmt::Display for Located<'de, T>
where
    T: core::fmt::Display,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(
            f,
            "at source location {}:{}, {}",
            self.line, self.column, self.value
        )?;
        if let Some(line) = self.source_line() {
            writeln!(f, "  | {}", line)?;
            writeln!(f, "    {: <1$}^", "", self.column)?;
        }
        Ok(())
    }
}
