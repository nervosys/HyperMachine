//! Print what the process sandbox can actually enforce on the machine running it.
//!
//! `Sandbox::controls()` is probed rather than assumed, so the answer is a
//! property of this host and not of the documentation. That distinction is the
//! whole design: a caller asking for "no network" gets a refusal where it cannot
//! be given, instead of a run that silently had one.
//!
//! ```text
//! cargo run -p hv2-sandbox --example what_this_host_enforces
//! ```
//!
//! Written because a downstream project was deciding whether to depend on this
//! crate and needed the answer for *its* platform, which is Windows. Reading the
//! table in `docs/SANDBOXES.md` would have been taking a document's word for
//! something the code will tell you directly.

use hv2_sandbox::{Control, ProcessSandbox, Sandbox};

fn main() {
    let sandbox = ProcessSandbox::new();
    let enforced = sandbox.controls();

    println!("backend: {}", sandbox.name());
    println!("host:    {} / {}", std::env::consts::OS, std::env::consts::ARCH);
    println!();

    let mut have = Vec::new();
    let mut missing = Vec::new();
    for control in Control::ALL {
        if enforced.enforces(control) {
            have.push(control);
        } else {
            missing.push(control);
        }
    }

    println!("enforced here ({}):", have.len());
    for control in &have {
        println!("  + {control}");
    }
    println!();
    println!("NOT enforced here ({}):", missing.len());
    for control in &missing {
        // The reason is the point. A backend that reports a gap without saying
        // why leaves an operator with nothing to change.
        match enforced.reason(*control) {
            Some(why) => println!("  - {control}: {why}"),
            None => println!("  - {control}"),
        }
    }

    println!();
    // The four that decide whether a workload is *contained* rather than merely
    // *bounded*. Resource limits stop a program using too much; only these stop
    // it reading your files or opening a socket.
    let containment = [
        Control::NetworkIsolation,
        Control::FilesystemIsolation,
        Control::ProcessIsolation,
        Control::NoNewPrivileges,
    ];
    let contained = containment.iter().filter(|c| enforced.enforces(**c)).count();
    println!(
        "containment: {contained} of {} — {}",
        containment.len(),
        if contained == containment.len() {
            "a boundary"
        } else if contained == 0 {
            "resource limits only; a workload here can read your files and open sockets"
        } else {
            "partial, which is the case worth reading carefully"
        }
    );
}
