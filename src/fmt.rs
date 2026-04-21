// helper to format strings nicely in error message
pub struct FormatIter<'a, I> {
    iter: core::cell::Cell<Option<I>>,
    sep: &'a str,
}

impl<'a, I> FormatIter<'a, I> {
    pub fn new(iter: I, sep: &'a str) -> Self {
        Self {
            iter: core::cell::Cell::new(Some(iter)),
            sep,
        }
    }
}

impl<'a, I> core::fmt::Display for FormatIter<'a, I>
where
    I: Iterator,
    I::Item: core::fmt::Display,
{
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        let iter = self.iter.take().expect("FormatIter used more than once");
        let mut first = true;
        for part in iter {
            let sep = if !first { self.sep } else { "" };
            write!(f, "{}{}", sep, part)?;
            first = false;
        }
        Ok(())
    }
}
