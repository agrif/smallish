use smallish::de::token;

static SOURCE: &str = r#"
some_enum a=bbb 25 4.0e2
bar {baz = 2}
"#;

fn main() {
    let mut tokenizer = token::Tokenizer::new(SOURCE);
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
