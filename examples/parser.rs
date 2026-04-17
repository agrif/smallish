use smallish::de;

static SOURCE: &str = r#"
some_enum a=bbb 25 4.0e2
bar {baz = 2}
"#;

fn main() {
    let mut state = [de::ParserState::default(); 8];
    let mut parser = de::Parser::list_from_str(SOURCE, &mut state);
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
