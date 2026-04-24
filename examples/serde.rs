use smallish::{from_str_escaped, Flavor};

#[derive(Clone, Debug, serde::Deserialize)]
enum Instr<'a> {
    Nop,
    Go {
        dir: Option<Direction>,
        #[serde(default)]
        label: &'a str,
    },
    Wait(u32),
    Draw(bool),
    Data(Vec<u32>),
}

#[derive(Clone, Debug, serde::Deserialize)]
enum Direction {
    North,
    South,
    East,
    West,
}

static SOURCE: &str = r#"
# wait before we begin
Nop
Wait 1000

# turn on drawing, then head north
Draw true
Go dir=North
Go dir=none label="going nowhere"

# some arbitrary data
Data 0x1234 0x5678
"#;

fn main() {
    let mut unescape_buffer = [0; 128];
    let instructions: Result<Vec<Instr>, _> =
        from_str_escaped(Flavor::List, SOURCE, &mut unescape_buffer);

    let instructions = instructions.unwrap_or_else(|e| {
        println!("error: {}", e);
        std::process::exit(1);
    });

    for instr in instructions {
        match instr {
            Instr::Nop => println!("doing nothing"),
            Instr::Go { dir, label } => println!("going {:?} with label {:?}", dir, label),
            Instr::Wait(time) => println!("waiting {}", time),
            Instr::Draw(true) => println!("draw on"),
            Instr::Draw(false) => println!("draw off"),
            Instr::Data(d) => println!("got data: {:?}", d),
        }
    }
}
