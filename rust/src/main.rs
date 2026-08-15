fn main() {
    let rc = gauntlet::cli::main(std::env::args_os());
    std::process::exit(rc);
}
