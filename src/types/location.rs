pub type LocResult<'de, T, E> = Result<Located<'de, T>, Located<'de, E>>;

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Located<'de, T> {
    pub source: Option<&'de [u8]>,
    pub line: usize,
    pub column: usize,
    pub offset: usize,
    pub value: T,
}

impl Located<'static, ()> {
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
    pub fn with_source<'a>(self, source: Option<&'a [u8]>) -> Located<'a, T> {
        Located { source, ..self }
    }

    pub fn without_source(self) -> Located<'static, T> {
        Located {
            source: None,
            ..self
        }
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

    pub fn pure(&self) -> Located<'de, ()> {
        self.wrap(())
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

    pub fn source_line(&self) -> Option<&'de [u8]> {
        let source = self.source?;
        let start = source[..self.offset]
            .iter()
            .rposition(|c| *c == b'\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let end = source[self.offset..]
            .iter()
            .position(|c| *c == b'\n')
            .map(|i| self.offset + i)
            .unwrap_or(source.len());
        Some(&source[start..end])
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
        if let Some(line) = self
            .source_line()
            .and_then(|s| core::str::from_utf8(s).ok())
        {
            writeln!(f, "  | {}", line)?;
            writeln!(f, "    {: <1$}^", "", self.column)?;
        }
        Ok(())
    }
}
