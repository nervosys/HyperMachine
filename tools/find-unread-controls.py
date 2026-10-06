"""Find controls that claim to act and nothing acts on.

The defect this repository has shipped most often is a security control that
reports a promise it does not keep, and rustc cannot see any of them, because
every one is `pub` and so exempt from dead-code analysis:

  * `FipsMode::Strict` was documented as "fail if non-FIPS operation
    attempted". Callers constructed it; nothing ever compared or matched it,
    so it behaved exactly like `Enabled`.
  * `CryptoError::AlgorithmNotApproved` was defined and never returned.
  * Five module headers said a module "enforces" or "wires" something that no
    code outside the module called (found by hand, 2026-09-22).

`tools/find-unreachable-states.py` covers a related shape (a state a guard
demands that nothing constructs). This covers the three above:

  A. Refusals nothing returns: in a `pub enum ...Error`, a refusal-shaped
     variant (`...Denied`, `...NotApproved`, `...Forbidden`, `...Violation`,
     `...Exceeded`, `...Revoked`, ...) with no construction site anywhere.
     Every error variant was 190 hits, mostly spare error kinds; a refusal
     that is never raised is the ones that matter, a control that cannot say
     no.
  B. Choices nothing acts on: in a `pub enum ...Mode`, `...Policy` or
     `...Level` that *is* read somewhere, a variant that no `==`, `!=`, match
     arm, `matches!` or `if let` ever names -- whether or not anything builds
     it. `FipsMode::Strict` was built by nothing and read by nothing before
     it was fixed; requiring a construction site missed it. A wildcard arm may be handling it deliberately; the point is
     that nothing *says* so.
  C. Claims without callers: a `//!` module header sentence saying the
     module enforces, wires, consults or intercepts a backticked item, where
     that item is defined in the file and referenced from no other file.

Same conventions as its sibling. A match arm is not construction. Every
known-good exception sits in ACCEPTED with its reason. Exit status is 1 only
for something new. Every hit is read by hand before it is believed.

Run `python3 tools/find-unread-controls.py --self-test` to prove each rule
fires on a planted example.
"""
import os
import re
import sys
from collections import defaultdict

ROOT = 'crates'

enum_re = re.compile(r'pub enum (\w+)\s*\{')
variant_re = re.compile(r'^\s*([A-Z]\w*)\s*(?:\{|\(|=|,|$)')
path_re = re.compile(r'\b(\w+)::(\w+)\b')
CLAIM_VERBS = re.compile(r'\b(enforc\w*|wires?|wired|consults?|intercepts?)\b', re.I)
BACKTICK = re.compile(r'`([A-Za-z_][A-Za-z0-9_:]*)`')
DEF_RE = r'\b(?:struct|enum|trait|fn|type|const|static|mod)\s+{name}\b'

CHOICE_SUFFIXES = ('Mode', 'Policy', 'Level')
REFUSAL = re.compile(
    r'(NotApproved|Denied|Deny|Forbidden|Unauthori[sz]ed|Refused|Violation|'
    r'Blocked|Rejected|NotPermitted|Disallowed|Unverified|Untrusted|Revoked|'
    r'Expired|InvalidSignature|InvalidToken|InvalidMac|Tamper|Exceeded|Limit)'
)


def load(root):
    sources = []
    for base, dirs, files in os.walk(root):
        dirs[:] = [d for d in dirs if d not in ('target', '.git')]
        for f in files:
            if f.endswith('.rs'):
                path = os.path.join(base, f)
                try:
                    text = open(path, encoding='utf-8').read()
                except Exception:
                    continue
                # One separator everywhere, then sorted: every platform reads
                # the same paths in the same order and reports them alike.
                sources.append((path.replace(os.sep, '/'), text))
    return sorted(sources)


def collect_enums(sources):
    """Every enum definition, keyed by (path, name).

    Keyed by definition, not by name: 32 enum names are defined in more than
    one file. Keying by name let whichever file was walked last win, and walk
    order differs between platforms, so Linux and Windows analysed different
    enums. Merging same-named enums was no better for rule B: one enum's reads
    made another's unread variants look like findings.
    """
    enums, by_name, defined_in = {}, defaultdict(list), defaultdict(list)
    for path, text in sources:
        lines = text.split('\n')
        for i, line in enumerate(lines):
            m = enum_re.search(line)
            if not m:
                continue
            name = m.group(1)
            variants, depth = [], 0
            for j in range(i, min(i + 300, len(lines))):
                depth += lines[j].count('{') - lines[j].count('}')
                if j > i:
                    vm = variant_re.match(lines[j])
                    if vm and depth == 1:
                        variants.append(vm.group(1))
                if depth <= 0 and j > i:
                    break
            if variants:
                key = (path, name)
                enums[key] = variants
                by_name[name].append(key)
                defined_in[path].append(key)
    return enums, by_name, defined_in


def classify(sources, enums, by_name, defined_in):
    """Count, per (definition, variant), where it is built and where it is read.

    `Name::Variant` cannot say which same-named definition it means, so it
    counts for every definition that has the variant: that can hide a finding,
    never invent one. `Self::Variant` resolves within its own file.
    """
    built, read = defaultdict(int), defaultdict(int)
    for path, text in sources:
        local = defined_in.get(path, [])
        for line in text.split('\n'):
            stripped = line.strip()
            if stripped.startswith('//') or stripped.startswith('#['):
                continue
            for m in path_re.finditer(line):
                qualifier, variant = m.group(1), m.group(2)
                if qualifier == 'Self':
                    owners = [k for k in local if variant in enums[k]]
                    if len(owners) != 1:
                        continue
                else:
                    owners = [k for k in by_name.get(qualifier, []) if variant in enums[k]]
                    if not owners:
                        continue
                before = line[:m.start()].rstrip()
                after = line[m.end():].lstrip()
                # Skip past a tuple/struct pattern's bindings to what follows.
                tail = re.sub(r'^(\([^)]*\)|\{[^}]*\})', '', after).lstrip()
                reading = (
                    before.endswith('==') or before.endswith('!=')
                    or tail.startswith('=>') or tail.startswith('|')
                    or before.endswith('|')
                    or 'matches!(' in before
                    or re.search(r'\b(if|while)\s+let\s*$', before)
                    or (before.endswith('let') and tail.startswith('='))
                )
                for key in owners:
                    if reading:
                        read[(key, variant)] += 1
                    else:
                        built[(key, variant)] += 1
    return built, read


def rule_a(enums, built):
    return [
        (name, v, path)
        for (path, name), variants in sorted(enums.items())
        if name.endswith('Error')
        for v in variants
        if REFUSAL.search(v) and built[((path, name), v)] == 0
    ]


def rule_b(enums, built, read):
    out = []
    for (path, name), variants in sorted(enums.items()):
        if not name.endswith(CHOICE_SUFFIXES):
            continue
        key = (path, name)
        if not any(read[(key, v)] for v in variants):
            continue
        for v in variants:
            if read[(key, v)] == 0:
                out.append((name, v, path))
    return out


def rule_c(sources):
    texts = dict(sources)
    out = []
    for path, text in sources:
        header = []
        for line in text.split('\n'):
            s = line.strip()
            if s.startswith('//!'):
                header.append(s[3:].strip())
            elif s and not s.startswith('#!['):
                break
        # Sentences, joined across wrapped lines.
        for sentence in re.split(r'(?<=[.!?])\s+', ' '.join(header)):
            if not CLAIM_VERBS.search(sentence):
                continue
            for ident in BACKTICK.findall(sentence):
                name = ident.split('::')[-1].split('(')[0]
                if not name or not re.search(DEF_RE.format(name=re.escape(name)), text):
                    continue            # not defined here: a claim about something else
                word = re.compile(r'\b' + re.escape(name) + r'\b')
                elsewhere = [p for p, t in texts.items() if p != path and word.search(t)]
                if not elsewhere:
                    out.append((path, name, sentence[:110]))
    return out


# Reviewed and accepted, each with the reason it is not a defect. An entry
# that does not say why is an entry nobody can check.
ACCEPTED = {
    'MissedOccurrencePolicy::CatchUp': (
        'The fall-through of `== MissedOccurrencePolicy::Coalesce` in '
        'IntervalSchedule::due_occurrences: every path but Coalesce returns '
        'oldest-first bounded batches, and a test sets CatchUp explicitly.'),
    # --- A: refusals nothing returns -----------------------------------------
    'RegistryError::Denied': (
        'Image admission refuses through AdmissionDecision::Denied, a separate '
        'enum, and is tested. This error variant is left over.'),
    'CryptoError::InvalidSignature': (
        'Verification reports a bad signature as Ok(false), the documented '
        'contract of rsa_verify/ecdsa_verify/ml_dsa_verify.'),
    'SecureBootError::PolicyViolation': (
        'Secure boot refuses through VerificationResult, not an error; the '
        'verdicts are tested, including forgeries.'),
    'ToolError::PermissionDenied': (
        'Tool permissions are enforced in the MCP layer by capabilities and VM '
        'ownership (McpServer), which refuse with its own errors.'),
    'OrchestrationError::RateLimitExceeded': (
        'Orchestration has no rate-limit setting to enforce; the variant is left '
        'over. MCP calls are rate-limited in McpServer.'),
    'AgentError::ResourceLimit': (
        'Known and documented: limits.rs decides resource limits and nothing '
        'intercepts on them. The real gate is capabilities and VM ownership.'),
    'SriovError::PermissionDenied': (
        'SR-IOV has no permission model to enforce; the variant is left over.'),
    'TaskError::RetryLimitExceeded': (
        'The limit is enforced: RetryPolicy::allows_retry stops the loop at '
        'max_retries. The last attempt\'s error is returned instead of this one.'),
    'LearningError::CapacityExceeded': (
        'The experience buffer is a ring at capacity; overwriting the oldest '
        'entry is the design, so nothing is refused.'),
    'PageWalkError::AccessViolation': (
        'The debugger page walker reports permission bits and never enforces '
        'them. Nothing uses it as a guard.'),
    'VmxInstructionError::VmEntryEventsBlockedMovSs': (
        'The full SDM error-number catalogue for nested VMX; the emulator '
        'returns the codes it detects. MOV-SS blocking on nested entry is not '
        'modelled: a fidelity gap, not an isolation one.'),
    # --- B: choices nothing acts on --------------------------------------------
    'FipsMode::Enabled': (
        'Acted on as `!= FipsMode::Disabled` (self-tests and the DRBG) and by '
        'not being Strict (no refusal). Handled, just never named.'),
    'SecureBootMode::Setup': (
        'Recorded, and nothing behaves differently for it. In UEFI, Setup vs User '
        'decides whether key-database updates need authentication; here add_db '
        'and add_kek accept anything in any mode. Contained because '
        'SecureBootManager is a host-side API no guest reaches; revisit before '
        'exposing UEFI variable services to a guest.'),
    'SecureBootMode::User': 'As SecureBootMode::Setup.',
    'SecureBootMode::Deployed': 'As SecureBootMode::Setup; documented on the variant.',
    'SecureBootMode::Audit': (
        'Enforces like every mode but Disabled, and now says so. Its docs and a '
        'comment in verify() had claimed "log but do not enforce"; the code '
        'enforced. Fail-closed; audit-only admission is deliberately not built.'),
    'TimerMode::Reserved': 'The reserved LAPIC timer-mode encoding, correctly never acted on.',
    'CpuMode::LongModeCompatibility': (
        'WHPX CpuMode is an observation, not a choice: the backend reports the '
        "guest's mode to callers and tests, and nothing in hv2 is meant to branch "
        'on it. Matched only because the name ends in Mode.'),
    'CpuMode::LongMode64Bit': 'As CpuMode::LongModeCompatibility.',
    'MsiDestMode::Physical': 'The `else` of `== MsiDestMode::Logical`.',
    'TimerMode::OneShot': 'The `else` of `== TimerMode::Periodic` in the LAPIC timer.',
    'VlanMode::None': 'Falls through the Access/Trunk match: no tag, no filtering.',
    # --- C: claims without callers ------------------------------------------------
    'crates/hv2-agent/src/sandbox.rs:check_permission': (
        'The claim is a denial ("It does not install seccomp filters ... '
        'check_permission ..."): the header says what the module does NOT do.'),
    'crates/hv2-agent/src/sandbox.rs:validate_resources': 'As sandbox.rs:check_permission.',
    'crates/hv2-api/src/permission_middleware.rs:permission_handler': (
        'True and stated: the header opens "Nothing installs this". Tracked in the '
        'implementation plan (wire or remove permission_middleware).'),
}


def run(root):
    sources = load(root)
    enums, by_name, defined_in = collect_enums(sources)
    built, read = classify(sources, enums, by_name, defined_in)
    findings = (
        [('A', f'{n}::{v}', p, 'error variant nothing returns') for n, v, p in rule_a(enums, built)]
        + [('B', f'{n}::{v}', p, 'choice nothing acts on') for n, v, p in rule_b(enums, built, read)]
        + [('C', f'{p}:{name}', p, f'claim without a caller: "{s}"') for p, name, s in rule_c(sources)]
    )
    return enums, findings


def self_test():
    import tempfile
    planted = '''//! This module enforces `PlantedGuard` on every request.
pub struct PlantedGuard;
pub enum PlantedError {
    Returned,
    RequestDenied,
}
pub enum PlantedMode {
    Off,
    Strict,
}
pub fn f(m: PlantedMode) -> Result<(), PlantedError> {
    if m == PlantedMode::Off { return Ok(()) }
    Err(PlantedError::Returned)
}
pub fn g() -> PlantedMode { PlantedMode::Strict }
'''
    with tempfile.TemporaryDirectory() as d:
        os.makedirs(os.path.join(d, 'x', 'src'))
        open(os.path.join(d, 'x', 'src', 'lib.rs'), 'w').write(planted)
        _, findings = run(d)
    keys = {(r, k.split('/')[-1].split('\\')[-1]) for r, k, _, _ in findings}
    want = {
        ('A', 'PlantedError::RequestDenied'),
        ('B', 'PlantedMode::Strict'),
        ('C', 'lib.rs:PlantedGuard'),
    }
    missing = want - keys
    unexpected = {k for k in keys if k[1].startswith(('PlantedError::Returned', 'PlantedMode::Off'))}
    for w in sorted(want):
        print(('ok    ' if w in keys else 'MISSED') + f' rule {w[0]} fires on {w[1]}')
    for u in sorted(unexpected):
        print(f'WRONG rule {u[0]} fired on {u[1]}')
    return 1 if missing or unexpected else 0


if __name__ == '__main__':
    if '--self-test' in sys.argv:
        raise SystemExit(self_test())
    enums, findings = run(ROOT)
    fresh = [f for f in findings if f[1] not in ACCEPTED]
    print('enums scanned:', len(enums))
    print('controls that claim to act and nothing acts on: %d (%d reviewed, %d new)'
          % (len(findings), len(findings) - len(fresh), len(fresh)))
    print()
    for rule, key, path, what in findings:
        mark = '    ' if key in ACCEPTED else 'NEW '
        print(f'{mark}{rule}  {key:<48} {what}  [{path}]')
    if fresh:
        print()
        print('Each NEW line is a control nothing exercises, or a false positive this')
        print('script cannot see through. Read it, then fix it or add it to ACCEPTED')
        print('with the reason. Do not add one without a reason.')
    raise SystemExit(1 if fresh else 0)
