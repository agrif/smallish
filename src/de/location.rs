#[derive(Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Located<'de, T> {
    pub source: Option<&'de str>,
    pub line: usize,
    pub column: usize,
    offset: usize,
    value: T,
}

impl<'de> Located<'de, ()> {
    pub fn from_source(source: &'de str) -> Self {
        Located {
            source: Some(source),
            ..Default::default()
        }
    }
}

impl<'de, T> Located<'de, T> {
    pub fn advance(&mut self, start: &'de str, end: &'de str) {
        if start.len() < end.len() {
            return;
        }

        let new = &start[..start.len() - end.len()];
        let amt = new.len();
        let mut newlines = 0;
        let mut last_newline = None;
        for (i, _) in new.bytes().enumerate().filter(|(_, c)| *c == b'\n') {
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

    pub fn replace<U>(self, value: U) -> (Located<'de, U>, T) {
        (
            Located {
                source: self.source,
                line: self.line,
                column: self.column,
                offset: self.offset,
                value,
            },
            self.value,
        )
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

    pub fn advance_and_wrap<U>(
        &mut self,
        start: &'de str,
        end: &'de str,
        value: U,
    ) -> Located<'de, U> {
        let wrapped = self.wrap(value);
        self.advance(start, end);
        wrapped
    }

    pub fn forget_source(self) -> Located<'static, T> {
        Located {
            source: None,
            ..self
        }
    }

    pub fn get_source_line(&self) -> Option<&'de str> {
        let source = self.source?;
        let start = source[..self.offset]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let end = source[self.offset..]
            .find('\n')
            .map(|i| self.offset + i)
            .unwrap_or(source.len());
        Some(&source[start..end])
    }
}

impl<'de, T, E> Located<'de, Result<T, E>> {
    pub fn transpose(self) -> Result<Located<'de, T>, Located<'de, E>> {
        let (loc, value) = self.replace(());
        match value {
            Ok(t) => Ok(loc.replace(t).0),
            Err(e) => Err(loc.replace(e).0),
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
            "{} at source location {}:{}",
            self.value, self.line, self.column
        )?;
        if let Some(line) = self.get_source_line() {
            writeln!(f, "  | {}", line)?;
            writeln!(f, "    {}^", " ".repeat(self.column))?;
        }
        Ok(())
    }
}
