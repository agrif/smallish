use smallish::de;

static SOURCE: &[u8] = br#"
some_enum a=bbb 25 4.0e2 [[0, 1], 2]
bar {baz = "hello\nworld"}
"#;

fn main() {
    let mut tokenizer = de::Tokenizer::new(SOURCE);
    loop {
        let tok = tokenizer.next();
        match tok {
            Ok(tok) => println!("{:?}", *tok),
            Err(e) => {
                println!("error: {}", e);
                break;
            }
        }
    }
}
