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
    SetOptions(Options<'a>),
    NewtypeTuple(((u8, u8), char)),
    Nop,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[allow(unused)]
struct Options<'a> {
    #[serde(default)]
    foo: DirSubOptions,
    bar: &'a str,
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
SetOptions bar="hello\nworld"
NewtypeTuple [0, 1] '\u{1f914}'
NewtypeTuple [0, 1] '🤔'
"#;

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
