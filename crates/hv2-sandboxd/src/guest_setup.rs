//! Guest setup that must work on an image with no shell.
//!
//! A supported kind of image (`build.sh --no-busybox`) holds only the guest
//! agent as PID 1, which links `/etc/resolv.conf` itself, plus static
//! binaries. Anything the daemon would run through `/bin/sh -c` has to have a
//! twin that goes through the agent's file operations instead.

use std::time::Duration;

use hv2_agent::AgentVM;

/// Where OpenSSL, curl and Python's ssl look for trusted CAs by default.
pub(crate) const CA_BUNDLE: &str = "/etc/ssl/certs/ca-certificates.crt";

/// Whether an exec failed because the guest image has no shell at all, which
/// is a supported kind of image, not a fault.
pub(crate) fn guest_has_no_shell(error: &str) -> bool {
    error.contains("could not start /bin/sh") && error.contains("No such file or directory")
}

/// What to tell a caller whose shell-form command met a guest with no shell.
/// `argv_hint` is whether the route has an argv form to point at; `error` is
/// the agent's own report, kept so the caller still sees what failed.
pub(crate) fn no_shell_message(argv_hint: bool, error: &str) -> String {
    let advice = if argv_hint {
        "this guest has no /bin/sh; send argv with an absolute program path instead of cmd"
    } else {
        "this guest's image has no /bin/sh, and this route runs its command through one"
    };
    format!("{advice} ({error})")
}

/// A line of the certificate's base64 body: enough to recognise it by.
pub(crate) fn ca_marker(ca: &str) -> Option<&str> {
    ca.lines().nth(1)
}

/// The trust bundle after the CA is added: `None` when the bundle already
/// holds it (nothing to write), else the bundle with the CA appended.
pub(crate) fn bundle_with_ca(bundle: &[u8], ca: &str) -> Option<Vec<u8>> {
    if let Some(marker) = ca_marker(ca) {
        let held = String::from_utf8_lossy(bundle).contains(marker);
        if held {
            return None;
        }
    }
    let mut out = bundle.to_vec();
    out.extend_from_slice(ca.as_bytes());
    Some(out)
}

/// Add the egress CA to the guest's trust bundle through the agent's file
/// operations, unless the bundle already holds it. An image with no shell
/// usually has no bundle either, which is then created.
pub(crate) async fn append_ca_without_shell(vm: &AgentVM, ca: &str) -> Result<(), String> {
    let wait = Duration::from_secs(10);
    let bundle = vm
        .read_file_in_guest(CA_BUNDLE, 4 << 20, wait)
        .await
        .unwrap_or_default();
    let Some(bundle) = bundle_with_ca(&bundle, ca) else {
        return Ok(());
    };
    vm.write_file_in_guest(CA_BUNDLE, bundle, wait)
        .await
        .map_err(|e| format!("configuring the guest's network: writing the CA: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CA: &str = "-----BEGIN CERTIFICATE-----\nQUJDREVGRw==\n-----END CERTIFICATE-----\n";

    #[test]
    fn a_bundle_without_the_ca_gets_it_appended() {
        let out = bundle_with_ca(b"old\n", CA).unwrap();
        assert_eq!(out, [b"old\n".as_slice(), CA.as_bytes()].concat());
        assert_eq!(bundle_with_ca(b"", CA).unwrap(), CA.as_bytes());
    }

    #[test]
    fn a_bundle_holding_the_ca_is_left_alone() {
        let held = bundle_with_ca(b"old\n", CA).unwrap();
        assert!(bundle_with_ca(&held, CA).is_none());
    }

    #[test]
    fn a_ca_with_no_body_line_is_always_appended() {
        assert!(bundle_with_ca(b"x", "short").is_some());
    }

    #[test]
    fn the_no_shell_message_points_at_argv_when_there_is_one() {
        let error = "could not start /bin/sh: No such file or directory";
        assert!(no_shell_message(true, error).contains("argv"));
        assert!(!no_shell_message(false, error).contains("argv"));
        assert!(no_shell_message(false, error).ends_with(&format!("({error})")));
    }
}
