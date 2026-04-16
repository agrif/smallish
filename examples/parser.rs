use smallish::de::parse;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let input = std::fs::read_to_string(&args[1]).unwrap();
    let mut parser = parse::Parser::<8>::list_from_str(&input);
    loop {
        let ev = parser.next();
        match ev {
            Ok(ev) => println!("{:?}", *ev),
            Err(e) => {
                println!("error: {}", e);
                break;
            }
        }
    }
}
