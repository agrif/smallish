// a macro to define Enum alongside EnumKind
// use: define_enum! { <normal enum definition> }
macro_rules! define_enum_with_kind {
    ($(#[$attr:meta])* $vis:vis enum $($tok:tt)+) => {
        define_enum_with_kind!{@only_define, $(#[$attr])* $vis enum $($tok)+}
        define_enum_with_kind!{@only_kind, $(#[$attr])* $vis enum $($tok)+}
    };

    (@only_define, $($tok:tt)+) => {
        $($tok)+
    };

    (@only_kind,
        $(#[$attr:meta])*
        $vis:vis enum $name:ident $(<$life:lifetime>)? {
            $(
                $(#[$variant_attr:meta])*
                $variant:ident $(($inner:ty))?
            ),*$(,)?
        }
    ) => {
        paste::paste! {
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            #[cfg_attr(feature = "defmt", derive(defmt::Format))]
            $vis enum [<$name Kind>] {
                $($variant,)*
            }

            impl $(<$life>)? $name $(<$life>)? {
                pub fn kind(&self) -> [<$name Kind>] {
                    match self {
                        $(Self::$variant $((define_enum_with_kind!(@pattern_ignore, $inner)))? => [<$name Kind>]::$variant,)*
                    }
                }
            }

            impl $(<$life>)? From<$name $(<$life>)?> for [<$name Kind>] {
                fn from(v: $name $(<$life>)?) -> [<$name Kind>] {
                    v.kind()
                }
            }
        }
    };

    (@pattern_ignore, $tok:tt) => {_};
}
