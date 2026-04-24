use std::io::{self, Read};

use smallish::{de, Flavor};

fn main() -> io::Result<()> {
    let mut flavor = Flavor::Value;

    let mut args = std::env::args();
    let _ = args.next();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--flavor" | "-f" => match args.next().as_ref() {
                Some(s) => match s.as_str() {
                    "value" => flavor = Flavor::Value,
                    "list" => flavor = Flavor::List,
                    "map" => flavor = Flavor::Map,
                    _ => Err(io::Error::other("bad flavor"))?,
                },
                None => Err(io::Error::other("no argument to -f/--flavor"))?,
            },
            _ => Err(io::Error::other(format!("unknown argument {}", arg)))?,
        }
    }

    let mut source = String::new();
    io::stdin().read_to_string(&mut source)?;

    let mut state = [de::ParserState::default(); 64];
    let parser = de::Parser::new(flavor, source.as_bytes(), &mut state);
    for ev in parser {
        match ev {
            Ok(ev) => println!("{:?}", *ev),
            Err(e) => {
                println!("error: {}", e);
                Err(io::Error::other("parser error"))?;
            }
        }
    }

    Ok(())
}
