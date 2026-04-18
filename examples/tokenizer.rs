use smallish::de;

static SOURCE: &str = r#"
some_enum a=bbb 25 4.0e2 [[0, 1], '🤔']
bar {baz = "hello\nworld"}
"#;

fn main() {
    let mut tokenizer = de::Tokenizer::new(SOURCE.as_bytes());
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
