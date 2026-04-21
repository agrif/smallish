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
slice of bytes. Both of these methods have a fixed recursion depth. If
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

## Special Types

There are a few special types that modify how a value is deserialized
by *smallish*, for example by annotating it with source location, or
opting out of string escapes for guaranteed zero-copy behavior. These
types are documented in the [types][] module.

 [types]: https://agrif.github.io/smallish/smallish/types/index.html

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
