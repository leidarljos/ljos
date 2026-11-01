//! `ljos-hud` process entry. `ljos hud` execs this binary.

fn main() {
    let code = match ljos_hud::run_cli() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("ljos-hud: {err:#}");
            1
        }
    };
    std::process::exit(code);
}
