use smallish::de::token;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let input = std::fs::read_to_string(&args[1]).unwrap();
    let mut tokenizer = token::Tokenizer::new(&input);
    loop {
        let tok = tokenizer.next().transpose();
        match tok {
            Ok(tok) => println!("{:?}", *tok),
            Err(e) => {
                println!("error: {}", e);
                break;
            }
        }
    }
}
