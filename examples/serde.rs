use smallish::de;

#[derive(Clone, Debug, serde::Deserialize)]
#[allow(unused)]
enum Instruction {
    Go { dir: Option<Direction> },
    Wait(u32),
    Draw(bool),
    Vec(u8, u8, u8),
    SetOptions(Options),
    Nop,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[allow(unused)]
struct Options {
    #[serde(default)]
    foo: u8,
    bar: u8,
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
Go dir=North
Go dir=null
SetOptions {foo=2, bar=8}
SetOptions {bar=8}
"#;

fn main() {
    match de::list_from_str::<Vec<Instruction>>(SOURCE) {
        Ok(instructions) => println!("{:#?}", instructions),
        Err(e) => println!("error: {}", e),
    }
}
