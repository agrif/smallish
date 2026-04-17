// a macro to define Enum alongside (optionally) EnumKind and as_variant()
// use: define_enum! { what_to_define, <normal enum definition> }
macro_rules! define_enum {
    (kind, $($tok:tt)+) => {
        define_enum!{@only_define, $($tok)+}
        define_enum!{@only_kind, $($tok)+}
    };

    (as_variant, $($tok:tt)+) => {
        define_enum!{@only_define, $($tok)+}
        define_enum!{@only_as_variant, $($tok)+}
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
                        $(Self::$variant $((define_enum!(@pattern_ignore, $inner)))? => [<$name Kind>]::$variant,)*
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

    (@only_as_variant,
        $(#[$attr:meta])*
        $vis:vis enum $name:ident $(<$life:lifetime>)? {
            $(
                $(#[$variant_attr:meta])*
                $variant:ident $(($inner:ty))?
            ),*$(,)?
        }
    ) => {
        paste::paste! {
            impl $(<$life>)? $name $(<$life>)? {
                $(
                    pub fn [<as_ $variant:snake>](&self) -> Option<define_enum!(@as_variant_type, $($inner)?)> {
                        if let Self::$variant $((define_enum!(@pattern, v, $inner)))? = self {
                            Some(define_enum!(@as_variant_value, v, $($inner)?))
                        } else {
                            None
                        }
                    }
                )*
            }
        }
    };

    (@as_variant_type, ) => {()};
    (@as_variant_type, $ty:ident) => {&$ty};
    (@as_variant_type, $ty:ty) => {$ty};
    (@as_variant_value, $var:ident, ) => {()};
    (@as_variant_value, $var:ident, $ty:ty) => {$var};
    (@pattern_ignore, $tok:tt) => {_};
    (@pattern, $var:ident, $tok:tt) => {$var};
}
