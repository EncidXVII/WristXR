mod bindings;
mod gates;
#[cfg(windows)]
mod gates_vr;
#[cfg(windows)]
mod run;

use std::process::ExitCode;

const REPORT: &str = "selftest-report.json";

fn main() -> ExitCode {
    if std::env::args().any(|a| a == "--selftest") {
        return selftest();
    }
    #[cfg(windows)]
    let result = run::run();
    #[cfg(not(windows))]
    let result: Result<(), String> = Err("Linux capture and input backends are not built yet.".into());
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn selftest() -> ExitCode {
    let mut results: Vec<_> = gates::GATES.iter().map(|(name, gate)| (*name, gate())).collect();
    #[cfg(windows)]
    if std::env::args().any(|a| a == "--vr") {
        results.push(("1+2+P live overlay", gates_vr::overlay_live()));
    }
    for (name, r) in &results {
        match r {
            Ok(()) => println!("PASS  {name}"),
            Err(e) => println!("FAIL  {name}: {e}"),
        }
    }
    let report = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "gates": results
            .iter()
            .map(|(n, r)| serde_json::json!({ "gate": n, "ok": r.is_ok(), "detail": r.as_ref().err() }))
            .collect::<Vec<_>>(),
    });
    let written = serde_json::to_vec_pretty(&report)
        .map_err(|e| e.to_string())
        .and_then(|b| std::fs::write(REPORT, b).map_err(|e| e.to_string()));
    match written {
        Ok(()) => println!("report: {REPORT}"),
        Err(e) => {
            eprintln!("could not write {REPORT}: {e}");
            return ExitCode::FAILURE;
        }
    }
    if results.iter().all(|(_, r)| r.is_ok()) { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_gates_pass() {
        let failures: Vec<String> =
            crate::gates::GATES.iter().filter_map(|(n, g)| g().err().map(|e| format!("{n}: {e}"))).collect();
        assert!(failures.is_empty(), "{failures:#?}");
    }
}
