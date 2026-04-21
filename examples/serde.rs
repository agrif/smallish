#[derive(Clone, Debug, serde::Deserialize)]
#[allow(unused)]
enum Instruction<'a> {
    Go {
        dir: Option<Direction>,
        #[serde(default)]
        opts: DirOptions,
    },
    Wait(u32),
    Draw(bool),
    Vec(u8, u8, u8),
    #[serde(borrow)]
    SetOptions(smallish::types::Located<'a, Options<'a>>),
    NewtypeTuple(((u8, u8), smallish::types::Located<'a, char>)),
    NewtypeOpt(Option<(u8, u8, u8)>),
    List(Vec<Direction>),
    Nop,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[allow(unused)]
struct Options<'a> {
    #[serde(default)]
    foo: DirSubOptions,
    bar: &'a str,
    #[serde(default)]
    baz: smallish::types::Escaped<&'a [u8]>,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
#[allow(unused)]
struct DirOptions {
    sub: DirSubOptions,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
#[allow(unused)]
struct DirSubOptions {
    flag: bool,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[allow(unused)]
enum Direction {
    North,
    South,
    East,
    West,
    Turnwise(u8),
}

static SOURCE: &str = r#"
Nop
Wait 1000
Draw true
Vec 0 1 2
Go dir=North
Go dir=(Turnwise 2)
Go dir=null opts={sub={flag=true}}
SetOptions foo={flag=false} bar="bar" madeup={this=2}
SetOptions bar="hello\nworld" baz=b"\n\n"
NewtypeTuple [0, 1] '\u{1f914}'
NewtypeTuple [0, 1] '🤔'
List North South (Turnwise 2)
NewtypeOpt null
NewtypeOpt [0, 1, 2]
List North South"#;

fn main() {
    let mut unescape_buffer = [0; 128];
    match smallish::from_str_escaped::<Vec<Instruction>>(
        smallish::Flavor::List,
        SOURCE,
        &mut unescape_buffer,
    ) {
        Ok(instructions) => println!("{:#?}", instructions),
        Err(e) => println!("error: {}", e),
    }
}
