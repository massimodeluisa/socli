//! The terminal binary. The reader lives in the library.

fn main() {
    #[cfg(feature = "native")]
    socli::cli::main();
    #[cfg(not(feature = "native"))]
    {
        eprintln!("socli: this build has no terminal. The browser demo is the library.");
        std::process::exit(2);
    }
}
