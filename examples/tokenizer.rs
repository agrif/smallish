use smallish::de;

static SOURCE: &str = r#"
some_enum a=bbb 25 4.0e2 [[0, 1], '🤔']
bar {baz = "hello\nworld"}
"#;

fn main() {
    let tokenizer = de::Tokenizer::new(SOURCE.as_bytes());
    for tok in tokenizer {
        match tok {
            Ok(tok) => println!("{:?}", *tok),
            Err(e) => {
                println!("error: {}", e);
                break;
            }
        }
    }
}
