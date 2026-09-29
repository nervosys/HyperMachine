//! TPM 2.0 command dispatcher: a command buffer in, a response buffer out.
//!
//! [`VirtualTpm::execute`] takes one command in the TPM 2.0 wire format
//! (Library Specification, Parts 2 and 3) and returns the response a TPM
//! would. It is the interface a device model -- TIS or CRB -- forwards to. It
//! is not a device model itself: no guest can reach it until one exists.
//!
//! # What it answers
//!
//! `TPM2_Startup`, `TPM2_Shutdown`, `TPM2_SelfTest`, `TPM2_GetRandom`,
//! `TPM2_GetCapability` (algorithms, commands, PCR banks and fixed
//! properties), `TPM2_PCR_Read` and `TPM2_PCR_Extend`. Every other command
//! answers `TPM_RC_COMMAND_CODE`, as a TPM does for one it does not implement,
//! and `TPM_CAP_COMMANDS` lists only these, so a guest can tell in advance.
//!
//! # Sessions
//!
//! Only the password session, `TPM_RS_PW`, and only the empty password, which
//! is a PCR's `authValue`. There is no `TPM2_StartAuthSession`, so no HMAC or
//! policy sessions, and no audit or encryption sessions on commands that need
//! no authorization: those must use `TPM_ST_NO_SESSIONS`.

use super::{HashAlgorithm, StartupType, TpmCommandCode, TpmResponseCode, TpmState, VirtualTpm};
use std::sync::atomic::Ordering;

const TPM_ST_NO_SESSIONS: u16 = 0x8001;
const TPM_ST_SESSIONS: u16 = 0x8002;
const HEADER_LEN: usize = 10;

/// The largest command this TPM accepts, and the largest response it sends
/// (`TPM_PT_MAX_COMMAND_SIZE`, `TPM_PT_MAX_RESPONSE_SIZE`).
pub const MAX_COMMAND_SIZE: usize = 4096;

const TPM_RS_PW: u32 = 0x4000_0009;
const TPM_RH_NULL: u32 = 0x4000_0007;
/// `continueSession`: a password session always reports it set.
const CONTINUE_SESSION: u8 = 0x01;

const PCR_COUNT: u32 = 24;
/// `PCR_SELECT_MIN` and `PCR_SELECT_MAX`: 24 PCRs need three bytes.
const PCR_SELECT_SIZE: u8 = 3;
/// A `TPML_DIGEST` carries at most eight digests, so `TPM2_PCR_Read` returns
/// at most eight PCRs and clears the rest from the selection it echoes.
const MAX_PCR_READ_DIGESTS: usize = 8;
/// `HASH_COUNT`: the most entries a `TPML_PCR_SELECTION` or
/// `TPML_DIGEST_VALUES` may hold, one per hash algorithm.
const HASH_COUNT: u32 = 5;
/// The largest digest any bank here produces: SHA-512.
const MAX_DIGEST: u16 = 64;

const TPM_CAP_ALGS: u32 = 0x0000_0000;
const TPM_CAP_COMMANDS: u32 = 0x0000_0002;
const TPM_CAP_PCRS: u32 = 0x0000_0005;
const TPM_CAP_TPM_PROPERTIES: u32 = 0x0000_0006;

/// `TPMA_ALGORITHM.hash`.
const TPMA_ALGORITHM_HASH: u32 = 1 << 2;
/// `TPMA_CC.cHandles` starts at bit 25.
const TPMA_CC_C_HANDLES_SHIFT: u32 = 25;

/// Hash algorithms this TPM can compute, in `TPM_ALG_ID` order.
///
/// SHA-1 and SM3 are left out: [`super::PcrBank`] refuses to extend with
/// either, so advertising them would promise banks that cannot measure.
const IMPLEMENTED_HASHES: [HashAlgorithm; 3] = [
    HashAlgorithm::Sha256,
    HashAlgorithm::Sha384,
    HashAlgorithm::Sha512,
];

/// The commands [`VirtualTpm::execute`] implements, in command-code order,
/// with the number of handles each takes.
const IMPLEMENTED_COMMANDS: [(TpmCommandCode, u32); 7] = [
    (TpmCommandCode::SelfTest, 0),
    (TpmCommandCode::Startup, 0),
    (TpmCommandCode::Shutdown, 0),
    (TpmCommandCode::GetCapability, 0),
    (TpmCommandCode::GetRandom, 0),
    (TpmCommandCode::PcrRead, 0),
    (TpmCommandCode::PcrExtend, 1),
];

/// Fixed properties (`TPM_PT_*` in the `PT_FIXED` group), in property order.
///
/// `TPM_PT_MANUFACTURER` is "HMVT", which is not a TCG-registered vendor ID:
/// this TPM does not claim to be anyone's product.
const FIXED_PROPERTIES: [(u32, u32); 17] = [
    (0x100, u32::from_be_bytes(*b"2.0\0")), // TPM_PT_FAMILY_INDICATOR
    (0x101, 0),                             // TPM_PT_LEVEL
    (0x102, 138),                           // TPM_PT_REVISION: 1.38
    (0x105, u32::from_be_bytes(*b"HMVT")),  // TPM_PT_MANUFACTURER
    (0x106, u32::from_be_bytes(*b"Hype")),  // TPM_PT_VENDOR_STRING_1
    (0x107, u32::from_be_bytes(*b"rMac")),  // TPM_PT_VENDOR_STRING_2
    (0x108, u32::from_be_bytes(*b"hine")),  // TPM_PT_VENDOR_STRING_3
    (0x109, u32::from_be_bytes(*b"vTPM")),  // TPM_PT_VENDOR_STRING_4
    (0x10A, 0),                             // TPM_PT_VENDOR_TPM_TYPE
    (0x10B, 0),                             // TPM_PT_FIRMWARE_VERSION_1
    (0x10C, 0),                             // TPM_PT_FIRMWARE_VERSION_2
    (0x10D, 1024),                          // TPM_PT_INPUT_BUFFER
    (0x112, PCR_COUNT),                     // TPM_PT_PCR_COUNT
    (0x113, PCR_SELECT_SIZE as u32),        // TPM_PT_PCR_SELECT_MIN
    (0x11E, MAX_COMMAND_SIZE as u32),       // TPM_PT_MAX_COMMAND_SIZE
    (0x11F, MAX_COMMAND_SIZE as u32),       // TPM_PT_MAX_RESPONSE_SIZE
    (0x120, MAX_DIGEST as u32),             // TPM_PT_MAX_DIGEST
];

type Rc<T> = Result<T, TpmResponseCode>;

/// A cursor over big-endian TPM fields. Running short is `TPM_RC_INSUFFICIENT`.
struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Rc<&'a [u8]> {
        if self.buf.len() < n {
            return Err(TpmResponseCode::Insufficient);
        }
        let (head, rest) = self.buf.split_at(n);
        self.buf = rest;
        Ok(head)
    }

    fn u8(&mut self) -> Rc<u8> {
        Ok(self.bytes(1)?[0])
    }

    fn u16(&mut self) -> Rc<u16> {
        let b = self.bytes(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Rc<u32> {
        let b = self.bytes(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// A `TPM2B_*`: a u16 size, then that many bytes.
    fn tpm2b(&mut self) -> Rc<&'a [u8]> {
        let n = self.u16()?;
        self.bytes(n as usize)
    }

    /// Every parameter has been read; anything left is `TPM_RC_SIZE`.
    fn finish(self) -> Rc<()> {
        if self.buf.is_empty() {
            Ok(())
        } else {
            Err(TpmResponseCode::Size)
        }
    }
}

/// One `TPMS_PCR_SELECTION`.
struct PcrSelection {
    hash: u16,
    select: [u8; PCR_SELECT_SIZE as usize],
}

fn read_pcr_selections(r: &mut Reader<'_>) -> Rc<Vec<PcrSelection>> {
    let count = r.u32()?;
    if count > HASH_COUNT {
        return Err(TpmResponseCode::Size);
    }
    let mut out = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let hash = r.u16()?;
        if HashAlgorithm::from_algorithm_id(hash).is_none() {
            return Err(TpmResponseCode::BadHash);
        }
        if r.u8()? != PCR_SELECT_SIZE {
            return Err(TpmResponseCode::BadParam);
        }
        let mut select = [0u8; PCR_SELECT_SIZE as usize];
        select.copy_from_slice(r.bytes(PCR_SELECT_SIZE as usize)?);
        out.push(PcrSelection { hash, select });
    }
    Ok(out)
}

fn write_pcr_selections(out: &mut Vec<u8>, selections: &[PcrSelection]) {
    out.extend_from_slice(&(selections.len() as u32).to_be_bytes());
    for s in selections {
        out.extend_from_slice(&s.hash.to_be_bytes());
        out.push(PCR_SELECT_SIZE);
        out.extend_from_slice(&s.select);
    }
}

/// A response: header, then `body`. `body` for a `TPM_ST_SESSIONS` response
/// already holds its `parameterSize` and authorization area.
fn response(tag: u16, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + body.len());
    out.extend_from_slice(&tag.to_be_bytes());
    out.extend_from_slice(&((HEADER_LEN + body.len()) as u32).to_be_bytes());
    out.extend_from_slice(&(TpmResponseCode::Success as u32).to_be_bytes());
    out.extend_from_slice(body);
    out
}

/// An error response is a bare header, always tagged `TPM_ST_NO_SESSIONS`.
fn error_response(rc: TpmResponseCode) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN);
    out.extend_from_slice(&TPM_ST_NO_SESSIONS.to_be_bytes());
    out.extend_from_slice(&(HEADER_LEN as u32).to_be_bytes());
    out.extend_from_slice(&(rc as u32).to_be_bytes());
    out
}

/// A `TpmResponseCode` from the host-side API, as a `Result`.
fn check(rc: TpmResponseCode) -> Rc<()> {
    match rc {
        TpmResponseCode::Success => Ok(()),
        rc => Err(rc),
    }
}

/// A command that authorizes nothing must not carry sessions: this TPM has
/// no audit or encryption sessions to honour them with.
fn no_sessions(tag: u16) -> Rc<()> {
    if tag == TPM_ST_SESSIONS {
        return Err(TpmResponseCode::AuthContext);
    }
    Ok(())
}

/// The authorization area of a command with one authorized handle whose
/// `authValue` is empty. Returns the attributes to echo in the response.
fn read_empty_password_session(tag: u16, r: &mut Reader<'_>) -> Rc<u8> {
    if tag != TPM_ST_SESSIONS {
        return Err(TpmResponseCode::AuthMissing);
    }
    let size = r.u32()? as usize;
    let mut area = Reader {
        buf: r.bytes(size)?,
    };
    if area.u32()? != TPM_RS_PW {
        return Err(TpmResponseCode::SessionHandle);
    }
    // nonceCaller: a password session has no use for one.
    area.tpm2b()?;
    area.u8()?;
    // The password. A PCR's authValue is empty, so any other is wrong.
    if !area.tpm2b()?.is_empty() {
        return Err(TpmResponseCode::AuthFail);
    }
    // More sessions than the one authorized handle.
    if !area.buf.is_empty() {
        return Err(TpmResponseCode::AuthContext);
    }
    Ok(CONTINUE_SESSION)
}

impl VirtualTpm {
    /// Execute one TPM 2.0 command and return its response.
    ///
    /// Never fails: every error, from a truncated header to a wrong password,
    /// is a response carrying the TPM's response code, which is what a guest
    /// expects. See the module docs for the commands implemented.
    pub fn execute(&self, command: &[u8]) -> Vec<u8> {
        self.dispatch(command).unwrap_or_else(error_response)
    }

    fn dispatch(&self, command: &[u8]) -> Rc<Vec<u8>> {
        if command.len() < HEADER_LEN || command.len() > MAX_COMMAND_SIZE {
            return Err(TpmResponseCode::CommandSize);
        }
        let mut r = Reader { buf: command };
        let tag = r.u16()?;
        let size = r.u32()?;
        let code = r.u32()?;
        if tag != TPM_ST_NO_SESSIONS && tag != TPM_ST_SESSIONS {
            return Err(TpmResponseCode::BadTag);
        }
        if size as usize != command.len() {
            return Err(TpmResponseCode::CommandSize);
        }
        if code != TpmCommandCode::Startup as u32 && self.state() != TpmState::Ready {
            return Err(TpmResponseCode::Initialize);
        }
        let implemented = IMPLEMENTED_COMMANDS
            .iter()
            .map(|(cc, _)| *cc)
            .find(|cc| *cc as u32 == code);
        match implemented {
            Some(TpmCommandCode::Startup) => self.cmd_startup(tag, r),
            Some(TpmCommandCode::Shutdown) => self.cmd_shutdown(tag, r),
            Some(TpmCommandCode::SelfTest) => self.cmd_self_test(tag, r),
            Some(TpmCommandCode::GetRandom) => self.cmd_get_random(tag, r),
            Some(TpmCommandCode::GetCapability) => self.cmd_get_capability(tag, r),
            Some(TpmCommandCode::PcrRead) => self.cmd_pcr_read(tag, r),
            Some(TpmCommandCode::PcrExtend) => self.cmd_pcr_extend(tag, r),
            _ => Err(TpmResponseCode::CommandCode),
        }
    }

    fn cmd_startup(&self, tag: u16, mut r: Reader<'_>) -> Rc<Vec<u8>> {
        no_sessions(tag)?;
        let startup_type = match r.u16()? {
            0 => StartupType::Clear,
            1 => StartupType::State,
            _ => return Err(TpmResponseCode::BadParam),
        };
        r.finish()?;
        check(self.startup(startup_type))?;
        Ok(response(TPM_ST_NO_SESSIONS, &[]))
    }

    fn cmd_shutdown(&self, tag: u16, mut r: Reader<'_>) -> Rc<Vec<u8>> {
        no_sessions(tag)?;
        if r.u16()? > 1 {
            return Err(TpmResponseCode::BadParam);
        }
        r.finish()?;
        check(self.shutdown())?;
        Ok(response(TPM_ST_NO_SESSIONS, &[]))
    }

    fn cmd_self_test(&self, tag: u16, mut r: Reader<'_>) -> Rc<Vec<u8>> {
        no_sessions(tag)?;
        let full_test = match r.u8()? {
            0 => false,
            1 => true,
            _ => return Err(TpmResponseCode::BadParam),
        };
        r.finish()?;
        check(self.self_test(full_test))?;
        Ok(response(TPM_ST_NO_SESSIONS, &[]))
    }

    /// `bytesRequested` is capped at the largest digest, as a TPM caps it.
    fn cmd_get_random(&self, tag: u16, mut r: Reader<'_>) -> Rc<Vec<u8>> {
        no_sessions(tag)?;
        let requested = r.u16()?.min(MAX_DIGEST);
        r.finish()?;
        let bytes = self.get_random(requested as usize)?;
        let mut body = Vec::with_capacity(2 + bytes.len());
        body.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
        body.extend_from_slice(&bytes);
        Ok(response(TPM_ST_NO_SESSIONS, &body))
    }

    fn cmd_get_capability(&self, tag: u16, mut r: Reader<'_>) -> Rc<Vec<u8>> {
        no_sessions(tag)?;
        let capability = r.u32()?;
        let property = r.u32()?;
        let count = r.u32()? as usize;
        r.finish()?;

        // Every list below is a table sorted by its key: return the entries
        // from `property` on, at most `count`, and say whether more remain.
        fn page<T: Copy>(
            table: &[T],
            key: impl Fn(&T) -> u32,
            from: u32,
            count: usize,
        ) -> (bool, Vec<T>) {
            let rest: Vec<T> = table.iter().copied().filter(|e| key(e) >= from).collect();
            let more = rest.len() > count;
            (more, rest.into_iter().take(count).collect())
        }

        let mut data = Vec::new();
        let more = match capability {
            TPM_CAP_ALGS => {
                let (more, algs) = page(
                    &IMPLEMENTED_HASHES,
                    |a| a.algorithm_id() as u32,
                    property,
                    count,
                );
                data.extend_from_slice(&(algs.len() as u32).to_be_bytes());
                for alg in algs {
                    data.extend_from_slice(&alg.algorithm_id().to_be_bytes());
                    data.extend_from_slice(&TPMA_ALGORITHM_HASH.to_be_bytes());
                }
                more
            }
            TPM_CAP_COMMANDS => {
                let (more, commands) =
                    page(&IMPLEMENTED_COMMANDS, |(cc, _)| *cc as u32, property, count);
                data.extend_from_slice(&(commands.len() as u32).to_be_bytes());
                for (cc, handles) in commands {
                    let attributes = (cc as u32 & 0xFFFF) | (handles << TPMA_CC_C_HANDLES_SHIFT);
                    data.extend_from_slice(&attributes.to_be_bytes());
                }
                more
            }
            TPM_CAP_PCRS => {
                // Every allocated bank, every PCR selected: the allocation.
                let banks = self.pcr_banks.read();
                let mut ids: Vec<u16> = banks.keys().map(|a| a.algorithm_id()).collect();
                ids.sort_unstable();
                let selections: Vec<PcrSelection> = ids
                    .into_iter()
                    .map(|hash| PcrSelection {
                        hash,
                        select: [0xFF; PCR_SELECT_SIZE as usize],
                    })
                    .collect();
                write_pcr_selections(&mut data, &selections);
                false
            }
            TPM_CAP_TPM_PROPERTIES => {
                let (more, props) = page(&FIXED_PROPERTIES, |(p, _)| *p, property, count);
                data.extend_from_slice(&(props.len() as u32).to_be_bytes());
                for (p, v) in props {
                    data.extend_from_slice(&p.to_be_bytes());
                    data.extend_from_slice(&v.to_be_bytes());
                }
                more
            }
            _ => return Err(TpmResponseCode::BadParam),
        };

        let mut body = Vec::with_capacity(5 + data.len());
        body.push(u8::from(more));
        body.extend_from_slice(&capability.to_be_bytes());
        body.extend_from_slice(&data);
        self.command_count.fetch_add(1, Ordering::Relaxed);
        Ok(response(TPM_ST_NO_SESSIONS, &body))
    }

    /// Returns the selected PCRs, lowest bank-order first, up to eight; the
    /// echoed selection has a bit set exactly for each digest returned. A
    /// bank that is not allocated contributes nothing and comes back cleared.
    fn cmd_pcr_read(&self, tag: u16, mut r: Reader<'_>) -> Rc<Vec<u8>> {
        no_sessions(tag)?;
        let mut selections = read_pcr_selections(&mut r)?;
        r.finish()?;

        // One lock across counter and values: an extend holds the write lock
        // while it bumps the counter, so the two always agree.
        let banks = self.pcr_banks.read();
        let counter = self.pcr_update_counter.load(Ordering::Acquire);
        let mut digests: Vec<&[u8]> = Vec::new();
        for s in &mut selections {
            let bank = HashAlgorithm::from_algorithm_id(s.hash).and_then(|a| banks.get(&a));
            for pcr in 0..PCR_COUNT as usize {
                let (byte, bit) = (pcr / 8, 1u8 << (pcr % 8));
                if s.select[byte] & bit == 0 {
                    continue;
                }
                match bank.and_then(|b| b.read(pcr)) {
                    Some(value) if digests.len() < MAX_PCR_READ_DIGESTS => digests.push(value),
                    _ => s.select[byte] &= !bit,
                }
            }
        }

        let mut body = Vec::new();
        body.extend_from_slice(&counter.to_be_bytes());
        write_pcr_selections(&mut body, &selections);
        body.extend_from_slice(&(digests.len() as u32).to_be_bytes());
        for d in &digests {
            body.extend_from_slice(&(d.len() as u16).to_be_bytes());
            body.extend_from_slice(d);
        }
        drop(banks);
        self.command_count.fetch_add(1, Ordering::Relaxed);
        Ok(response(TPM_ST_NO_SESSIONS, &body))
    }

    /// Extends one PCR in every allocated bank the command carries a digest
    /// for, as one change: all digests are checked before any is applied.
    /// A digest for an unallocated bank is ignored, as the specification
    /// says, and `TPM_RH_NULL` makes the whole command a no-op.
    fn cmd_pcr_extend(&self, tag: u16, mut r: Reader<'_>) -> Rc<Vec<u8>> {
        let handle = r.u32()?;
        let attributes = read_empty_password_session(tag, &mut r)?;
        let count = r.u32()?;
        if count > HASH_COUNT {
            return Err(TpmResponseCode::Size);
        }
        let mut digests = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let alg = HashAlgorithm::from_algorithm_id(r.u16()?).ok_or(TpmResponseCode::BadHash)?;
            digests.push((alg, r.bytes(alg.output_size())?));
        }
        r.finish()?;

        if handle != TPM_RH_NULL {
            if handle >= PCR_COUNT {
                return Err(TpmResponseCode::BadPcr);
            }
            let index = handle as usize;
            let mut banks = self.pcr_banks.write();
            // `PcrBank::extend` refuses only an out-of-range index (checked
            // above) or a bank it has no hash for. Such a bank should never
            // have been allocated; refuse before extending any other, so a
            // failed command changes nothing.
            if digests
                .iter()
                .any(|(alg, _)| banks.contains_key(alg) && !IMPLEMENTED_HASHES.contains(alg))
            {
                return Err(TpmResponseCode::Failure);
            }
            let mut changed = false;
            for (alg, digest) in &digests {
                if let Some(bank) = banks.get_mut(alg) {
                    changed |= bank.extend(index, digest);
                }
            }
            if changed {
                self.pcr_update_counter.fetch_add(1, Ordering::AcqRel);
            }
        }

        // parameterSize (no response parameters), then one response session:
        // an empty nonceTPM, the attributes, an empty HMAC.
        let mut body = Vec::with_capacity(9);
        body.extend_from_slice(&0u32.to_be_bytes());
        body.extend_from_slice(&0u16.to_be_bytes());
        body.push(attributes);
        body.extend_from_slice(&0u16.to_be_bytes());
        self.command_count.fetch_add(1, Ordering::Relaxed);
        Ok(response(TPM_ST_SESSIONS, &body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STARTUP_CLEAR: [u8; 12] = [
        0x80, 0x01, 0x00, 0x00, 0x00, 0x0C, 0x00, 0x00, 0x01, 0x44, 0x00, 0x00,
    ];
    const SUCCESS: [u8; 10] = [0x80, 0x01, 0x00, 0x00, 0x00, 0x0A, 0x00, 0x00, 0x00, 0x00];

    fn command(tag: u16, code: u32, params: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&((HEADER_LEN + params.len()) as u32).to_be_bytes());
        out.extend_from_slice(&code.to_be_bytes());
        out.extend_from_slice(params);
        out
    }

    fn cmd(code: TpmCommandCode, params: &[u8]) -> Vec<u8> {
        command(TPM_ST_NO_SESSIONS, code as u32, params)
    }

    fn rc(response: &[u8]) -> u32 {
        u32::from_be_bytes(response[6..10].try_into().unwrap())
    }

    fn started() -> VirtualTpm {
        let tpm = VirtualTpm::new();
        assert_eq!(tpm.execute(&STARTUP_CLEAR), SUCCESS);
        tpm
    }

    /// A password session with `password`, as `TPM2_PCR_Extend` carries it.
    fn password_session(password: &[u8]) -> Vec<u8> {
        let mut s = Vec::new();
        s.extend_from_slice(&TPM_RS_PW.to_be_bytes());
        s.extend_from_slice(&0u16.to_be_bytes());
        s.push(CONTINUE_SESSION);
        s.extend_from_slice(&(password.len() as u16).to_be_bytes());
        s.extend_from_slice(password);
        let mut area = (s.len() as u32).to_be_bytes().to_vec();
        area.extend_from_slice(&s);
        area
    }

    fn extend(pcr: u32, session: &[u8], digests: &[(u16, &[u8])]) -> Vec<u8> {
        let tag = if session.is_empty() {
            TPM_ST_NO_SESSIONS
        } else {
            TPM_ST_SESSIONS
        };
        let mut p = pcr.to_be_bytes().to_vec();
        p.extend_from_slice(session);
        p.extend_from_slice(&(digests.len() as u32).to_be_bytes());
        for (alg, d) in digests {
            p.extend_from_slice(&alg.to_be_bytes());
            p.extend_from_slice(d);
        }
        command(tag, TpmCommandCode::PcrExtend as u32, &p)
    }

    fn selection(hash: u16, select: [u8; 3]) -> Vec<u8> {
        let mut p = 1u32.to_be_bytes().to_vec();
        p.extend_from_slice(&hash.to_be_bytes());
        p.push(3);
        p.extend_from_slice(&select);
        p
    }

    fn sha256(data: &[u8]) -> Vec<u8> {
        use ic_core::traits::Digest;
        ic_hash::Sha256::digest(data).to_vec()
    }

    #[test]
    fn startup_clear_answers_the_reference_bytes() {
        let tpm = VirtualTpm::new();
        assert_eq!(tpm.execute(&STARTUP_CLEAR), SUCCESS);
        assert_eq!(tpm.state(), TpmState::Ready);
    }

    #[test]
    fn a_second_startup_is_refused() {
        let tpm = started();
        assert_eq!(
            tpm.execute(&STARTUP_CLEAR),
            error_response(TpmResponseCode::Initialize)
        );
    }

    #[test]
    fn commands_before_startup_are_refused() {
        let tpm = VirtualTpm::new();
        assert_eq!(
            tpm.execute(&cmd(TpmCommandCode::GetRandom, &[0, 8])),
            error_response(TpmResponseCode::Initialize)
        );
    }

    #[test]
    fn get_random_returns_the_count_asked_for_up_to_the_largest_digest() {
        let tpm = started();
        let resp = tpm.execute(&cmd(TpmCommandCode::GetRandom, &[0, 8]));
        assert_eq!(rc(&resp), 0);
        assert_eq!(resp.len(), 20);
        assert_eq!(&resp[2..6], &20u32.to_be_bytes());
        assert_eq!(&resp[10..12], &8u16.to_be_bytes());

        let resp = tpm.execute(&cmd(TpmCommandCode::GetRandom, &[0x10, 0]));
        assert_eq!(&resp[10..12], &MAX_DIGEST.to_be_bytes());
        assert_eq!(resp.len(), 12 + MAX_DIGEST as usize);
    }

    /// The old generator was a counter from a fixed seed, so two vTPMs gave
    /// the same bytes: every guest would have been handed the same "entropy".
    #[test]
    fn two_tpms_do_not_share_a_random_stream() {
        let a = started().get_random(32).unwrap();
        let b = started().get_random(32).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn extend_then_read_is_the_hash_chain() {
        let tpm = started();
        let digest = sha256(b"measured");
        let resp = tpm.execute(&extend(7, &password_session(b""), &[(0x000B, &digest)]));
        // parameterSize 0, empty nonceTPM, continueSession, empty HMAC.
        assert_eq!(
            resp,
            [0x80, 0x02, 0, 0, 0, 0x13, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01, 0, 0]
        );

        let resp = tpm.execute(&cmd(
            TpmCommandCode::PcrRead,
            &selection(0x000B, [0x80, 0, 0]),
        ));
        assert_eq!(rc(&resp), 0);
        let mut expected = 1u32.to_be_bytes().to_vec(); // pcrUpdateCounter
        expected.extend_from_slice(&selection(0x000B, [0x80, 0, 0]));
        expected.extend_from_slice(&1u32.to_be_bytes());
        expected.extend_from_slice(&32u16.to_be_bytes());
        let mut chain = vec![0u8; 32];
        chain.extend_from_slice(&digest);
        expected.extend_from_slice(&sha256(&chain));
        assert_eq!(&resp[10..], &expected[..]);
    }

    #[test]
    fn pcr_read_returns_at_most_eight_and_clears_the_rest() {
        let tpm = started();
        // PCRs 0-9 selected; 8 and 9 do not fit.
        let resp = tpm.execute(&cmd(
            TpmCommandCode::PcrRead,
            &selection(0x000B, [0xFF, 0x03, 0]),
        ));
        assert_eq!(rc(&resp), 0);
        let body = &resp[10..];
        assert_eq!(&body[4..8], &1u32.to_be_bytes());
        assert_eq!(&body[11..14], &[0xFF, 0x00, 0x00]);
        assert_eq!(&body[14..18], &8u32.to_be_bytes());
        assert_eq!(body.len(), 18 + 8 * 34);
    }

    #[test]
    fn pcr_read_of_an_unallocated_bank_returns_nothing() {
        let tpm = started();
        let resp = tpm.execute(&cmd(
            TpmCommandCode::PcrRead,
            &selection(0x000C, [0x01, 0, 0]),
        ));
        assert_eq!(rc(&resp), 0);
        let body = &resp[10..];
        assert_eq!(&body[11..14], &[0, 0, 0]);
        assert_eq!(&body[14..18], &0u32.to_be_bytes());
    }

    #[test]
    fn pcr_extend_requires_the_empty_password() {
        let tpm = started();
        let digest = [0u8; 32];
        let digests: &[(u16, &[u8])] = &[(0x000B, &digest)];

        let resp = tpm.execute(&extend(0, &[], digests));
        assert_eq!(rc(&resp), TpmResponseCode::AuthMissing as u32);

        let resp = tpm.execute(&extend(0, &password_session(b"guess"), digests));
        assert_eq!(rc(&resp), TpmResponseCode::AuthFail as u32);

        let mut hmac_session = password_session(b"");
        hmac_session[4..8].copy_from_slice(&0x0200_0000u32.to_be_bytes());
        let resp = tpm.execute(&extend(0, &hmac_session, digests));
        assert_eq!(rc(&resp), TpmResponseCode::SessionHandle as u32);

        let mut two_sessions = password_session(b"");
        let one = two_sessions[4..].to_vec();
        two_sessions.extend_from_slice(&one);
        let n = (two_sessions.len() - 4) as u32;
        two_sessions[..4].copy_from_slice(&n.to_be_bytes());
        let resp = tpm.execute(&extend(0, &two_sessions, digests));
        assert_eq!(rc(&resp), TpmResponseCode::AuthContext as u32);

        assert_eq!(
            tpm.pcr_read(HashAlgorithm::Sha256, 0).unwrap(),
            vec![0u8; 32]
        );
        assert_eq!(tpm.pcr_update_counter(), 0);
    }

    #[test]
    fn pcr_extend_rejects_bad_handles_and_honours_null() {
        let tpm = started();
        let digest = [1u8; 32];
        let digests: &[(u16, &[u8])] = &[(0x000B, &digest)];
        let session = password_session(b"");

        let resp = tpm.execute(&extend(24, &session, digests));
        assert_eq!(rc(&resp), TpmResponseCode::BadPcr as u32);
        assert_eq!(rc(&tpm.execute(&extend(TPM_RH_NULL, &session, digests))), 0);
        assert_eq!(tpm.pcr_update_counter(), 0);
        assert_eq!(rc(&tpm.execute(&extend(23, &session, digests))), 0);
        assert_eq!(tpm.pcr_update_counter(), 1);
    }

    /// A digest for a bank that is not allocated is ignored, and changes
    /// nothing: no counter bump for a command that extended no register.
    #[test]
    fn pcr_extend_ignores_unallocated_banks() {
        let tpm = started();
        let digest = [1u8; 48];
        let resp = tpm.execute(&extend(3, &password_session(b""), &[(0x000C, &digest)]));
        assert_eq!(rc(&resp), 0);
        assert_eq!(tpm.pcr_update_counter(), 0);
        assert_eq!(
            tpm.pcr_read(HashAlgorithm::Sha256, 3).unwrap(),
            vec![0u8; 32]
        );
    }

    /// A bank this build cannot hash for fails the command before any other
    /// bank is touched.
    #[test]
    fn pcr_extend_is_all_or_nothing() {
        let tpm = started();
        tpm.add_pcr_bank(HashAlgorithm::Sha1);
        let sha256_digest = [1u8; 32];
        let sha1_digest = [1u8; 20];
        let resp = tpm.execute(&extend(
            3,
            &password_session(b""),
            &[(0x000B, &sha256_digest), (0x0004, &sha1_digest)],
        ));
        assert_eq!(rc(&resp), TpmResponseCode::Failure as u32);
        assert_eq!(
            tpm.pcr_read(HashAlgorithm::Sha256, 3).unwrap(),
            vec![0u8; 32]
        );
        assert_eq!(tpm.pcr_update_counter(), 0);
    }

    #[test]
    fn startup_clear_after_shutdown_resets_the_pcrs() {
        let tpm = started();
        assert_eq!(
            tpm.pcr_extend(HashAlgorithm::Sha256, 0, b"x"),
            TpmResponseCode::Success
        );
        assert_eq!(
            tpm.execute(&cmd(TpmCommandCode::Shutdown, &[0, 0])),
            SUCCESS
        );
        assert_eq!(tpm.execute(&STARTUP_CLEAR), SUCCESS);
        assert_eq!(
            tpm.pcr_read(HashAlgorithm::Sha256, 0).unwrap(),
            vec![0u8; 32]
        );
        assert_eq!(tpm.pcr_update_counter(), 0);
    }

    #[test]
    fn malformed_commands_get_the_matching_response_code() {
        let tpm = started();
        let get_random = cmd(TpmCommandCode::GetRandom, &[0, 8]);

        let mut bad_tag = get_random.clone();
        bad_tag[1] = 0x03;
        assert_eq!(rc(&tpm.execute(&bad_tag)), TpmResponseCode::BadTag as u32);

        let mut bad_size = get_random.clone();
        bad_size[5] = 0x0D;
        assert_eq!(
            rc(&tpm.execute(&bad_size)),
            TpmResponseCode::CommandSize as u32
        );
        assert_eq!(
            rc(&tpm.execute(&[0x80, 0x01])),
            TpmResponseCode::CommandSize as u32
        );
        let oversized = vec![0u8; MAX_COMMAND_SIZE + 1];
        assert_eq!(
            rc(&tpm.execute(&oversized)),
            TpmResponseCode::CommandSize as u32
        );

        let short = cmd(TpmCommandCode::GetRandom, &[0]);
        assert_eq!(
            rc(&tpm.execute(&short)),
            TpmResponseCode::Insufficient as u32
        );

        let long = cmd(TpmCommandCode::GetRandom, &[0, 8, 0]);
        assert_eq!(rc(&tpm.execute(&long)), TpmResponseCode::Size as u32);

        let with_sessions = command(TPM_ST_SESSIONS, TpmCommandCode::GetRandom as u32, &[0, 8]);
        assert_eq!(
            rc(&tpm.execute(&with_sessions)),
            TpmResponseCode::AuthContext as u32
        );

        let unknown_hash = cmd(TpmCommandCode::PcrRead, &selection(0x0099, [1, 0, 0]));
        assert_eq!(
            rc(&tpm.execute(&unknown_hash)),
            TpmResponseCode::BadHash as u32
        );

        let fresh = VirtualTpm::new();
        let bad_startup = cmd(TpmCommandCode::Startup, &[0, 2]);
        assert_eq!(
            rc(&fresh.execute(&bad_startup)),
            TpmResponseCode::BadParam as u32
        );
    }

    #[test]
    fn unimplemented_commands_answer_command_code() {
        let tpm = started();
        for code in [
            TpmCommandCode::CreatePrimary,
            TpmCommandCode::Quote,
            TpmCommandCode::NvRead,
        ] {
            assert_eq!(
                tpm.execute(&cmd(code, &[])),
                error_response(TpmResponseCode::CommandCode)
            );
        }
        let unknown = command(TPM_ST_NO_SESSIONS, 0x2000_0000, &[]);
        assert_eq!(
            tpm.execute(&unknown),
            error_response(TpmResponseCode::CommandCode)
        );
    }

    fn get_capability(tpm: &VirtualTpm, cap: u32, property: u32, count: u32) -> Vec<u8> {
        let mut p = cap.to_be_bytes().to_vec();
        p.extend_from_slice(&property.to_be_bytes());
        p.extend_from_slice(&count.to_be_bytes());
        let resp = tpm.execute(&cmd(TpmCommandCode::GetCapability, &p));
        assert_eq!(rc(&resp), 0);
        resp[10..].to_vec()
    }

    #[test]
    fn get_capability_reports_the_pcr_allocation() {
        let tpm = started();
        let body = get_capability(&tpm, TPM_CAP_PCRS, 0, 1);
        assert_eq!(
            body,
            [0, 0, 0, 0, 5, 0, 0, 0, 1, 0, 0x0B, 3, 0xFF, 0xFF, 0xFF]
        );
    }

    #[test]
    fn get_capability_pages_properties_commands_and_algorithms() {
        let tpm = started();
        // TPM_PT_FAMILY_INDICATOR is "2.0", and more properties follow.
        let body = get_capability(&tpm, TPM_CAP_TPM_PROPERTIES, 0x100, 1);
        assert_eq!(
            body,
            [1, 0, 0, 0, 6, 0, 0, 0, 1, 0, 0, 1, 0, 0x32, 0x2E, 0x30, 0]
        );

        let body = get_capability(&tpm, TPM_CAP_TPM_PROPERTIES, 0x11F, 100);
        assert_eq!(body[0], 0);
        assert_eq!(&body[5..9], &2u32.to_be_bytes());

        // PCR_Extend is the last command, and the only one with a handle.
        let body = get_capability(&tpm, TPM_CAP_COMMANDS, 0x180, 100);
        assert_eq!(body, [0, 0, 0, 0, 2, 0, 0, 0, 1, 0x02, 0, 0x01, 0x82]);

        let body = get_capability(&tpm, TPM_CAP_ALGS, 0, 100);
        assert_eq!(&body[5..9], &3u32.to_be_bytes());
        assert_eq!(&body[9..15], &[0, 0x0B, 0, 0, 0, 0x04]);
    }

    #[test]
    fn unknown_capabilities_are_a_bad_parameter() {
        let tpm = started();
        let mut p = 0x99u32.to_be_bytes().to_vec();
        p.extend_from_slice(&[0; 8]);
        let resp = tpm.execute(&cmd(TpmCommandCode::GetCapability, &p));
        assert_eq!(rc(&resp), TpmResponseCode::BadParam as u32);
    }
}
