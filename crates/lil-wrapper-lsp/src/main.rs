use std::io;

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    let exit_code = match lil_wrapper_lsp::run_server(&mut input, &mut output) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("lil-wrapper-lsp: {error}");
            1
        }
    };
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}
