use smallish::de;

fn main() -> std::io::Result<()> {
    let mut source = String::new();
    use std::io::Read;
    std::io::stdin().read_to_string(&mut source)?;

    let tokenizer = de::Tokenizer::new(source.as_bytes());
    for tok in tokenizer {
        match tok {
            Ok(tok) => println!("{:?}", *tok),
            Err(e) => {
                println!("error: {}", e);
                Err(std::io::Error::other("tokenizer error"))?;
            }
        }
    }

    Ok(())
}
