# smallish

[![GitHub Action: Test](https://github.com/agrif/smallish/actions/workflows/test.yaml/badge.svg)](https://github.com/agrif/smallish/actions/workflows/test.yaml)
[![crates.io](https://img.shields.io/crates/v/smallish.svg)](https://crates.io/crates/smallish)
[![docs.rs](https://docs.rs/smallish/badge.svg)](https://docs.rs/smallish)

Lightweight, no-std, no-alloc syntax for configuration and scripting.

## Deserializing

*smallish* is designed to be used with [serde][] to parse lists of
short instructions. For example, you can put your instructions inside
an enumeration, and then parse them with [from_str][].

 [serde]: https://serde.rs/
 [from_str]: https://agrif.github.io/smallish/smallish/fn.from_str.html

```rust
use smallish::{Flavor, from_str};

#[derive(Debug, PartialEq, Eq, serde::Deserialize)]
enum Instr<'a> {
    Print { msg: &'a str },
    SetMinMax(u16, u16),
}

let source = r#"
Print msg="hello" # comments work
SetMinMax 20 60
"#;

let instrs: Vec<Instr> = from_str(Flavor::List, source).unwrap();
assert_eq!(instrs, &[Instr::Print{ msg: "hello" }, Instr::SetMinMax(20, 60)]);
```

It is also possible to use [from_slice][] if your source is a
bytestring. Both of these methods have a fixed recursion depth. If
your data type is very deeply nested, you should use [Deserializer][]
directly.

 [from_slice]: https://agrif.github.io/smallish/smallish/fn.from_slice.html
 [Deserializer]: https://agrif.github.io/smallish/smallish/de/struct.Deserializer.html

## Syntax

The details of *smallish* syntax are outlined in the [syntax][] module
documentation. This also includes many examples.

*smallish* comes in a few slightly different [Flavors][Flavor].

 [syntax]: https://agrif.github.io/smallish/smallish/syntax/index.html
 [Flavor]: https://agrif.github.io/smallish/smallish/enum.Flavor.html

## Escaping

*smallish* strings and bytestrings support the same escapes as
Rust. However, it needs a scratch buffer to parse strings with
escapes. This can be done with the [from_slice_escaped][] and
[from_str_escaped][] functions, or more directly with
[Deserializer][].

 [from_str_escaped]: https://agrif.github.io/smallish/smallish/fn.from_str_escaped.html
 [from_slice_escaped]: https://agrif.github.io/smallish/smallish/fn.from_slice_escaped.html

It is also possible to opt-out of unescaping by wrapping a string type
in [Escaped][]. This deserializes the string unmodified, with escapes
still intact, at which point you can choose to unescape it manually or
use it as-is.

 [Escaped]: https://agrif.github.io/smallish/smallish/types/struct.Escaped.html

## Locating Values and Errors

You can have *smallish* attach a source location to any value in your
type by wrapping it in [Located][]. This can be helpful to point
humans to where an error ocurred, for example.

 [Located]: https://agrif.github.io/smallish/smallish/types/struct.Located.html

Errors produced by *smallish* are always wrapped in [Located][]. Some
effort has gone into making them useful to humans even in an embedded
context.

## Feature Flags

These features are enabled by default:

 * **`custom-error-messages`** attaches a small space for custom
   error messages to the deserialization error type. This costs a
   small amount of space, but increases the usefulness of a few error
   messages.

These features are optional:

 * **`defmt`** derives `defmt::Format` for all types, and uses
   `defmt::panic!` and friends instead of their standard counterparts.

## License

Licensed under the [MIT license][LICENSE]. Unless stated otherwise,
any contributions to this work will also be licensed this way, with no
additional terms or conditions.

 [LICENSE]: ./LICENSE
