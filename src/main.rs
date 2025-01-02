use clap::Parser;

fn main() {
    let args = crashfeishu::Args::parse();
    if let Err(e) = crashfeishu::run(args) {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}
