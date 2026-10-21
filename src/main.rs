fn main() {
    if let Err(error) = alias_rs::run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
