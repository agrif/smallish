mod error;
pub use error::Error;

mod base;
pub use base::Deserializer;
use base::{Access, SmallishDe};

mod escaped;
mod inline;
mod located;

// helper to forward to self.de, in the same style as
// serde::forward_to_deserialize_any!
macro_rules! forward_to_inner_deserialize {
    // build one method, with arguments
    (@method, $func:ident<$l:tt, $v:ident>($($arg:ident : $ty:ty),*)) => {
        paste::paste! {
            #[inline]
            fn [<deserialize_ $func>]<$v>(
                self,
                $($arg: $ty,)*
                visitor: $v,
            ) -> Result<$v::Value, Self::Error>
            where
                $v: ::serde::de::Visitor<$l>,
            {
                self.de.[<deserialize_ $func>]($($arg,)* visitor)
            }
        }
    };

    // build one method, dispatching on type
    (@helper, unit_struct<$l:tt, $v:ident>) => {
        forward_to_inner_deserialize! {
            @method,
            unit_struct<$l, $v>(name: &'static str)
        }
    };
    (@helper, newtype_struct<$l:tt, $v:ident>) => {
        compile_error!("forwarding to inner for newtype_struct is almost always incorrect")
    };
    (@helper, tuple<$l:tt, $v:ident>) => {
        forward_to_inner_deserialize! {
            @method,
            tuple<$l, $v>(len: usize)
        }
    };
    (@helper, tuple_struct<$l:tt, $v:ident>) => {
        forward_to_inner_deserialize! {
            @method,
            tuple_struct<$l, $v>(name: &'static str, len: usize)
        }
    };
    (@helper, struct<$l:tt, $v:ident>) => {
        forward_to_inner_deserialize! {
            @method,
            struct<$l, $v>(name: &'static str, fields: &'static [&'static str])
        }
    };
    (@helper, enum<$l:tt, $v:ident>) => {
        forward_to_inner_deserialize! {
            @method,
            enum<$l, $v>(name: &'static str, variants: &'static [&'static str])
        }
    };

    // generic helper that only accepts visitor
    (@helper, $func:ident<$l:tt, $v:ident>) => {
        forward_to_inner_deserialize! { @method, $func<$l, $v>() }
    };

    // entry points
    (<$visitor:ident : Visitor<$lifetime:tt>> $($func:ident)*) => {
        $(forward_to_inner_deserialize! { @helper, $func<$lifetime, $visitor> })*
    };
    ($($func:ident)*) => {
        forward_to_inner_deserialize! { <V: Visitor<'de>> $($func)* }
    };
}

pub(crate) use forward_to_inner_deserialize;
