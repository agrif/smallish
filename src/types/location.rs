/// A [Result] where both the [Ok] and [Err] contain a [Located].
///
/// See also [Located::from_result] and [Located::to_result] for
/// flipping whether the [Located] is inside or outside of the
/// [Result].
pub type LocResult<'de, T, E> = Result<Located<'de, T>, Located<'de, E>>;

/// A wrapper that annotates a value with its source location.
///
/// *smallish* uses this type to provide context to errors. Its
/// implementation of [Display](core::fmt::Display) (and optionally
/// `defmt::Format`) will format the wrapped object with a reference
/// to the line of source attached to it.
///
/// ## Deserialization
///
/// This type modifies how deserialization works for the contained
/// type. Types wrapped in `Located` will be annotated with their
/// source location during deserialization. This can be useful to
/// provide context for errors that can only be recognized after the
/// entire object is deserialized.
///
/// ```
/// # use smallish::{Flavor, from_str, types::Located};
/// let r: Vec<Located<u8>> = from_str(Flavor::Value, "[10, 20, 30]").unwrap();
/// assert_eq!(r[1].line, 1);
/// assert_eq!(r[1].column, 5);
/// assert_eq!(*r[1], 20);
/// ```
///
/// The lifetime `'a` is the lifetime of the source itself. You can
/// drop this source with [without_source](Located::without_source) to
/// get a `'static` lifetime, at the cost of losing the full source
/// context.
#[derive(Clone, Copy, Eq, serde::Deserialize)]
#[serde(rename = "__smallish_magic_located__")]
pub struct Located<'a, T> {
    /// The entire source for the location this value is attached to.
    pub source: Option<&'a [u8]>,
    /// Line number, starting at 1.
    pub line: usize,
    /// Column number, starting at 0.
    pub column: usize,
    /// Offset into `self.source`.
    pub offset: usize,
    /// The value wrapped by this `Located`.
    pub value: T,
}

impl Located<'static, ()> {
    pub(crate) const SERDE_NAME: &'static str = "__smallish_magic_located__";

    /// Create a new, empty location, positioned at the start.
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
    /// Get the line of source attached to this value.
    ///
    /// This can fail if the source is missing or is not valid
    /// UTF-8. To get the bytes instead, see
    /// [source_line_bytes](Self::source_line_bytes).
    pub fn source_line(&self) -> Option<&'de str> {
        self.source_line_bytes()
            .and_then(|s| core::str::from_utf8(s).ok())
    }

    /// Get the line of source attached to this value, as bytes.
    ///
    /// This can fail if the source is missing. To get a `str`
    /// instead, see [source_line](Self::source_line).
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

    /// Wrap the given object in this location, returning a new `Located`.
    ///
    /// To replace the value without creating a new `Located`, see
    /// [replace](Self::replace).
    pub fn wrap<U>(&self, value: U) -> Located<'de, U> {
        Located {
            source: self.source,
            line: self.line,
            column: self.column,
            offset: self.offset,
            value,
        }
    }

    /// Replace the value contained in this `Located`.
    ///
    /// To create a new `Located` without modifying this one, see
    /// [wrap](Self::wrap).
    pub fn replace<U>(self, value: U) -> Located<'de, U> {
        self.map(|_| value)
    }

    /// Split this `Located` into a pure location and the contained value.
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

    /// Run a function on the contained value, and replace it with the result.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Located<'de, U> {
        Located {
            source: self.source,
            line: self.line,
            column: self.column,
            offset: self.offset,
            value: f(self.value),
        }
    }

    /// Attach a source string to this `Located`.
    pub fn with_source<'a>(self, source: Option<&'a [u8]>) -> Located<'a, T> {
        Located { source, ..self }
    }

    /// Drop the source string from this `Located`.
    ///
    /// This makes the context less useful, but it also drops the
    /// lifetime requirements.
    pub fn without_source(self) -> Located<'static, T> {
        Located {
            source: None,
            ..self
        }
    }
}

impl<'de, T, E> Located<'de, Result<T, E>> {
    /// Turn a `Result<Located<...>>` into a `Located<Result<...>>`.
    pub fn from_result(result: LocResult<'de, T, E>) -> Self {
        match result {
            Ok(t) => t.map(Ok),
            Err(e) => e.map(Err),
        }
    }

    /// Turn a `Located<Result<...>>` into a `Result<Located<...>>`.
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
