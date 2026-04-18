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
    NewtypeTuple(((u8, u8), u8)),
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
}

static SOURCE: &str = r#"
Nop
Wait 1000
Draw true
Vec 0 1 2
Go dir=North
Go dir=null opts={sub={flag=true}}
SetOptions foo={flag=false} bar="bar"
SetOptions bar="hello\nworld"
NewtypeTuple [0, 1] 2
"#;

fn main() {
    match smallish::from_str::<Vec<Instruction>>(smallish::Flavor::List, SOURCE) {
        Ok(instructions) => println!("{:#?}", instructions),
        Err(e) => println!("error: {}", e),
    }
}
