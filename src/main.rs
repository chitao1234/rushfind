fn main() {
    std::process::exit(findoxide::cli::run(std::env::args_os().skip(1)));
}
