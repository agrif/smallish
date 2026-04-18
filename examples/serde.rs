#[derive(Clone, Debug, serde::Deserialize)]
#[allow(unused)]
enum Instruction<'a> {
    Go {
        dir: Option<Direction>,
    },
    Wait(u32),
    Draw(bool),
    Vec(u8, u8, u8),
    #[serde(borrow)]
    SetOptions(Options<'a>),
    Nop,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[allow(unused)]
struct Options<'a> {
    #[serde(default)]
    foo: u8,
    bar: &'a str,
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
Vec [0, 1, 2]
Vec 0 1 2
Go {dir=North}
Go dir=null
SetOptions {foo=2, bar="bar"}
SetOptions {bar="hello\nworld"}
"#;

fn main() {
    match smallish::from_str::<Vec<Instruction>>(smallish::Flavor::List, SOURCE) {
        Ok(instructions) => println!("{:#?}", instructions),
        Err(e) => println!("error: {}", e),
    }
}
