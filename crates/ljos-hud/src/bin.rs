//! `ljos-hud` process entry. `ljos hud` execs this binary.

fn main() {
    ljos_cli::output::install_panic_hook();
    let code = match ljos_hud::run_cli() {
        Ok(code) => code,
        Err(err) => {
            ljos_cli::say!("ljos-hud: {err:#}");
            1
        }
    };
    std::process::exit(code);
}
