# smallish

[![GitHub Action: Test](https://github.com/agrif/smallish/actions/workflows/test.yaml/badge.svg)](https://github.com/agrif/smallish/actions/workflows/test.yaml)
[![crates.io](https://img.shields.io/crates/v/smallish.svg)](https://crates.io/crates/smallish)
[![docs.rs](https://docs.rs/smallish/badge.svg)](https://docs.rs/smallish)

Lightweight, no-std, no-alloc syntax for configuration and scripting.

## Quick Start

*smallish* is designed to be used with [serde][] to parse lists of
short instructions. For example, you can put your instructions inside
an enumeration.

 [serde]: https://serde.rs/

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

## Feature Flags

These features are enabled by default:

 * **`custom-error-messages`** attaches a small space for custom
   error messages to the deserialization error type. This costs a
   small amount of space, but increases the usefulness of a few error
   messages.

These features are optional:

 * **`defmt`** derives `defmt::Format` for all types, and uses
   `defmt::panic!` and friends instead of their standard counterparts.

## Escaping

*smallish* strings and bytestrings support the same escapes as
Rust. However, it needs a scratch buffer to parse strings with
escapes. This can be done with the [from_slice_escaped][] and
[from_str_escaped][] functions, or more directly with
[Deserializer][].

 [from_slice_escaped]: https://agrif.github.io/smallish/smallish/fn.from_slice_escaped.html
 [from_str_escaped]: https://agrif.github.io/smallish/smallish/fn.from_str_escaped.html
 [Deserializer]: https://agrif.github.io/smallish/smallish/de/struct.Deserializer.html

It is also possible to opt-out of unescaping by wrapping a string type
in [Escaped][]. This deserializes the string directly, with escapes
still intact, at which point you can choose to unescape it manually or
use it directly.

 [Escaped]: https://agrif.github.io/smallish/smallish/types/struct.Escaped.html

## Locating Values and Errors

You can have *smallish* attach a source location to any value in your
type by wrapping it in [Located][]. This can be helpful to point
humans to where an error ocurred, for example.

 [Located]: https://agrif.github.io/smallish/smallish/types/struct.Located.html

Errors produced by *smallish* are always wrapped in [Located][]. Some
effort has gone into making them useful to humans even in an embedded
context.

## License

Licensed under the [MIT license][LICENSE]. Unless stated otherwise,
any contributions to this work will also be licensed this way, with no
additional terms or conditions.

 [LICENSE]: ./LICENSE
