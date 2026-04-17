use smallish::de;

static SOURCE: &str = r#"
some_enum a=bbb 25 4.0e2
bar {baz = "hello\n"}
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
