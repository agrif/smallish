#![macro_use]

#[macro_use]
#[allow(unused)]
mod macros {
    // compile-time dispatch to defmt (or not!)
    // (adapted and mostly copied from embassy)

    // We check both feature = "defmt" and target_os = "none" so that
    // examples and tests can still compile and link on std.
    // If you are here because you are running defmt on std, I'm sorry.

    #[collapse_debuginfo(yes)]
    macro_rules! assert {
        ($($x:tt)*) => {
            {
                #[cfg(not(all(feature = "defmt", target_os = "none")))]
                ::core::assert!($($x)*);
                #[cfg(all(feature = "defmt", target_os = "none"))]
                ::defmt::assert!($($x)*);
            }
        };
    }

    #[collapse_debuginfo(yes)]
    macro_rules! assert_eq {
        ($($x:tt)*) => {
            {
                #[cfg(not(all(feature = "defmt", target_os = "none")))]
                ::core::assert_eq!($($x)*);
                #[cfg(all(feature = "defmt", target_os = "none"))]
                ::defmt::assert_eq!($($x)*);
            }
        };
    }

    #[collapse_debuginfo(yes)]
    macro_rules! assert_ne {
        ($($x:tt)*) => {
            {
                #[cfg(not(all(feature = "defmt", target_os = "none")))]
                ::core::assert_ne!($($x)*);
                #[cfg(all(feature = "defmt", target_os = "none"))]
                ::defmt::assert_ne!($($x)*);
            }
        };
    }

    #[collapse_debuginfo(yes)]
    macro_rules! debug_assert {
        ($($x:tt)*) => {
            {
                #[cfg(not(all(feature = "defmt", target_os = "none")))]
                ::core::debug_assert!($($x)*);
                #[cfg(all(feature = "defmt", target_os = "none"))]
                ::defmt::debug_assert!($($x)*);
            }
        };
    }

    #[collapse_debuginfo(yes)]
    macro_rules! debug_assert_eq {
        ($($x:tt)*) => {
            {
                #[cfg(not(all(feature = "defmt", target_os = "none")))]
                ::core::debug_assert_eq!($($x)*);
                #[cfg(all(feature = "defmt", target_os = "none"))]
                ::defmt::debug_assert_eq!($($x)*);
            }
        };
    }

    #[collapse_debuginfo(yes)]
    macro_rules! debug_assert_ne {
        ($($x:tt)*) => {
            {
                #[cfg(not(all(feature = "defmt", target_os = "none")))]
                ::core::debug_assert_ne!($($x)*);
                #[cfg(all(feature = "defmt", target_os = "none"))]
                ::defmt::debug_assert_ne!($($x)*);
            }
        };
    }

    #[collapse_debuginfo(yes)]
    macro_rules! todo {
        ($($x:tt)*) => {
            {
                #[cfg(not(all(feature = "defmt", target_os = "none")))]
                ::core::todo!($($x)*);
                #[cfg(all(feature = "defmt", target_os = "none"))]
                ::defmt::todo!($($x)*);
            }
        };
    }

    #[collapse_debuginfo(yes)]
    macro_rules! unreachable {
        ($($x:tt)*) => {
            {
                #[cfg(not(all(feature = "defmt", target_os = "none")))]
                ::core::unreachable!($($x)*);
                #[cfg(all(feature = "defmt", target_os = "none"))]
                ::defmt::unreachable!($($x)*);
            }
        };
    }

    #[collapse_debuginfo(yes)]
    macro_rules! panic {
        ($($x:tt)*) => {
            {
                #[cfg(not(all(feature = "defmt", target_os = "none")))]
                ::core::panic!($($x)*);
                #[cfg(all(feature = "defmt", target_os = "none"))]
                ::defmt::panic!($($x)*);
            }
        };
    }

    #[cfg(all(feature = "defmt", target_os = "none"))]
    #[collapse_debuginfo(yes)]
    macro_rules! unwrap {
        ($($x:tt)*) => {
            ::defmt::unwrap!($($x)*)
        };
    }

    #[cfg(not(all(feature = "defmt", target_os = "none")))]
    #[collapse_debuginfo(yes)]
    macro_rules! unwrap {
        ($arg:expr) => {
            $arg.unwrap()
        };
        ($arg:expr, $msg:expr) => {
            $arg.expect($msg)
        };
    }
}

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
        let iter = unwrap!(self.iter.take(), "FormatIter used more than once");
        let mut first = true;
        for part in iter {
            let sep = if !first { self.sep } else { "" };
            write!(f, "{}{}", sep, part)?;
            first = false;
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn format_iter() {
        extern crate alloc;
        use super::FormatIter;
        let iter = FormatIter::new(["hello", "world", "!"].into_iter(), ", ");
        let s = alloc::format!("{}", iter);
        assert_eq!(s, "hello, world, !");
    }
}
