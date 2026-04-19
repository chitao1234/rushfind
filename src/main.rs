fn main() {
    std::process::exit(rushfind::cli::run(std::env::args_os().skip(1)));
}
