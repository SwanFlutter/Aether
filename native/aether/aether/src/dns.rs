use std::net::SocketAddr;
use std::time::Duration;

use futures::stream::StreamExt;
use tokio::time::timeout_at;

use crate::error::{AetherError, Result};

pub const BOOTSTRAP_DNS: &[&str] = &["1.1.1.1:53", "1.0.0.1:53", "8.8.8.8:53"];
pub const ECH_HOSTS: &[&str] = &["cloudflare-ech.com", "crypto.cloudflare.com"];

const RR_HTTPS: u16 = 65;
const SVCPARAM_ECH: u16 = 5;

/// Hard ceiling on the whole ECHConfigList lookup, override with
/// `AETHER_ECH_BUDGET_MS`.
///
/// This lookup sits on the critical path of every single connect, before the
/// gateway is even chosen, so its cost is connect latency the user watches.
fn ech_budget() -> Duration {
    let ms = std::env::var("AETHER_ECH_BUDGET_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(2_500);
    Duration::from_millis(ms)
}

/// One lane of the race: a single (host, resolver) HTTPS/ECH query.
///
/// Deliberately a named `async fn` rather than an inline `async move` block
/// behind a closure. `futures::stream::iter(..).map(|(host, server)| async move
/// { .. })` makes rustc infer the closure signature as `fn((&str, &str)) ->
/// {async block}` for one specific pair of lifetimes instead of a
/// higher-ranked one, so the resulting future is never provably `Send` for any
/// lifetime. That is not a local problem: it propagates through
/// `fetch_ech_config` into every `spawn_job(..)` caller in `ffi.rs`, which
/// requires `Fut: Send`, and surfaces there as 14 confusing
/// "implementation of `FnOnce`/`Send` is not general enough" errors that never
/// name this file as the culprit. A named `async fn` taking `&'static str`
/// returns a concrete, unconditionally `Send` future, so there is no
/// higher-ranked lifetime left to infer.
async fn probe_ech_pair(host: &'static str, server: &'static str) -> Option<Vec<u8>> {
    let addr: SocketAddr = match server.parse() {
        Ok(a) => a,
        Err(_) => {
            log::debug!("ech bootstrap resolver {server} is not a valid socket address");
            return None;
        }
    };
    match query_ech(addr, host).await {
        Ok(ech) if !ech.is_empty() => {
            log::info!(
                "fetched ECHConfigList ({} bytes) for {host} via {server}",
                ech.len()
            );
            Some(ech)
        }
        Ok(_) => None,
        Err(e) => {
            log::debug!("ech bootstrap {host}@{server} failed: {e}");
            None
        }
    }
}

/// Looks up the ECHConfigList, racing every (host, resolver) pair at once.
///
/// ## 1.2.3-p3: this used to be nine seconds of pure connect latency
///
/// The old loop was strictly serial - two hosts times three resolvers, each with
/// its own 3 s timeout, so up to 18 s - and it stopped at the first success.
/// `cloudflare-ech.com` has no HTTPS record on these resolvers, so all three of
/// its attempts timed out before `crypto.cloudflare.com` was tried at all. The
/// field log measures the result exactly:
///
/// ```text
///   14:20:41.582  identity ready
///   14:20:50.731  fetched ECHConfigList (71 bytes) for crypto.cloudflare.com
/// ```
///
/// Nine seconds and 149 ms, on every attempt that had `--ech auto`, waiting for
/// a name that was never going to answer. Six concurrent UDP queries cost one
/// round trip instead of six timeouts, and the total is now bounded by
/// [`ech_budget`] rather than by how many dead names precede a live one.
pub async fn fetch_ech_config() -> Result<Vec<u8>> {
    let budget = ech_budget();

    let mut lanes = Vec::with_capacity(ECH_HOSTS.len() * BOOTSTRAP_DNS.len());
    for host in ECH_HOSTS {
        for server in BOOTSTRAP_DNS {
            lanes.push(probe_ech_pair(*host, *server));
        }
    }
    let width = lanes.len().max(1);

    let race = async move {
        let attempts = futures::stream::iter(lanes).buffer_unordered(width);
        tokio::pin!(attempts);
        while let Some(found) = attempts.next().await {
            if let Some(ech) = found {
                return Some(ech);
            }
        }
        None
    };

    match tokio::time::timeout(budget, race).await {
        Ok(Some(ech)) => Ok(ech),
        Ok(None) => Err(AetherError::Ech("no ECHConfigList resolved".into())),
        Err(_) => Err(AetherError::Ech(format!(
            "ECHConfigList lookup gave up after {} ms",
            budget.as_millis()
        ))),
    }
}

async fn query_ech(server: SocketAddr, host: &str) -> Result<Vec<u8>> {
    let (sock, _, _detour) = crate::upstream::bind_via_upstream(server).await?;

    let (query, id) = build_query(host, RR_HTTPS);
    sock.send(&query).await?;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let mut buf = [0u8; 4096];

    loop {
        let n = timeout_at(deadline, sock.recv(&mut buf))
            .await
            .map_err(|_| AetherError::Ech("dns timeout".into()))??;

        if !response_matches(&buf[..n], id, host, RR_HTTPS) {
            log::debug!("discarding an ech dns reply that does not match the query");
            continue;
        }

        return parse_https_ech(&buf[..n])
            .ok_or_else(|| AetherError::Ech("no ech svcparam".into()));
    }
}

pub fn response_matches(
    msg: &[u8],
    expected_id: u16,
    expected_name: &str,
    expected_qtype: u16,
) -> bool {
    if msg.len() < 12 {
        return false;
    }
    if u16::from_be_bytes([msg[0], msg[1]]) != expected_id {
        return false;
    }
    if msg[2] & 0x80 == 0 {
        return false;
    }
    if u16::from_be_bytes([msg[4], msg[5]]) != 1 {
        return false;
    }

    let mut pos = 12;
    for label in expected_name.split('.') {
        if label.is_empty() {
            continue;
        }
        let len = match msg.get(pos) {
            Some(value) => *value as usize,
            None => return false,
        };
        if len != label.len() {
            return false;
        }
        pos += 1;
        let end = match pos.checked_add(len) {
            Some(value) if value <= msg.len() => value,
            _ => return false,
        };
        if !msg[pos..end].eq_ignore_ascii_case(label.as_bytes()) {
            return false;
        }
        pos = end;
    }

    if msg.get(pos) != Some(&0) {
        return false;
    }
    pos += 1;

    if pos + 4 > msg.len() {
        return false;
    }

    u16::from_be_bytes([msg[pos], msg[pos + 1]]) == expected_qtype
}

fn build_query(name: &str, qtype: u16) -> (Vec<u8>, u16) {
    let mut q = Vec::with_capacity(32 + name.len());
    let id: u16 = rand::random();
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[0x01, 0x00]);
    q.extend_from_slice(&[0x00, 0x01]);
    q.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    for label in name.split('.') {
        if label.is_empty() {
            continue;
        }
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0x00);
    q.extend_from_slice(&qtype.to_be_bytes());
    q.extend_from_slice(&[0x00, 0x01]);
    (q, id)
}

fn parse_https_ech(msg: &[u8]) -> Option<Vec<u8>> {
    if msg.len() < 12 {
        return None;
    }
    let qd = u16::from_be_bytes([msg[4], msg[5]]) as usize;
    let an = u16::from_be_bytes([msg[6], msg[7]]) as usize;
    let mut pos = 12;

    for _ in 0..qd {
        pos = skip_name(msg, pos)?;
        pos = pos.checked_add(4)?;
    }

    for _ in 0..an {
        pos = skip_name(msg, pos)?;
        if pos + 10 > msg.len() {
            return None;
        }
        let rtype = u16::from_be_bytes([msg[pos], msg[pos + 1]]);
        let rdlen = u16::from_be_bytes([msg[pos + 8], msg[pos + 9]]) as usize;
        pos += 10;
        if pos + rdlen > msg.len() {
            return None;
        }
        if rtype == RR_HTTPS {
            if let Some(ech) = parse_svcparams_ech(msg, pos, rdlen) {
                return Some(ech);
            }
        }
        pos += rdlen;
    }
    None
}

fn parse_svcparams_ech(msg: &[u8], rdata_start: usize, rdlen: usize) -> Option<Vec<u8>> {
    let end = rdata_start + rdlen;
    if rdata_start + 2 > end {
        return None;
    }
    let mut p = skip_name(msg, rdata_start + 2)?;

    while p + 4 <= end {
        let key = u16::from_be_bytes([msg[p], msg[p + 1]]);
        let len = u16::from_be_bytes([msg[p + 2], msg[p + 3]]) as usize;
        p += 4;
        if p + len > end {
            return None;
        }
        if key == SVCPARAM_ECH {
            return Some(msg[p..p + len].to_vec());
        }
        p += len;
    }
    None
}

fn skip_name(buf: &[u8], mut pos: usize) -> Option<usize> {
    loop {
        let len = *buf.get(pos)?;
        if len & 0xc0 == 0xc0 {
            return Some(pos + 2);
        }
        if len == 0 {
            return Some(pos + 1);
        }
        pos += 1 + len as usize;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(id: u16, name: &str, qtype: u16, qr: bool, qdcount: u16) -> Vec<u8> {
        let mut msg = Vec::new();
        msg.extend_from_slice(&id.to_be_bytes());
        msg.push(if qr { 0x81 } else { 0x01 });
        msg.push(0x80);
        msg.extend_from_slice(&qdcount.to_be_bytes());
        msg.extend_from_slice(&1u16.to_be_bytes());
        msg.extend_from_slice(&[0, 0, 0, 0]);
        for label in name.split('.') {
            msg.push(label.len() as u8);
            msg.extend_from_slice(label.as_bytes());
        }
        msg.push(0);
        msg.extend_from_slice(&qtype.to_be_bytes());
        msg.extend_from_slice(&1u16.to_be_bytes());
        msg
    }

    #[test]
    fn build_query_reports_the_id_it_wrote() {
        let (query, id) = build_query("cloudflare-ech.com", RR_HTTPS);
        assert_eq!(u16::from_be_bytes([query[0], query[1]]), id);
    }

    #[test]
    fn accepts_a_reply_that_matches_the_query() {
        let msg = reply(0x1234, "cloudflare-ech.com", RR_HTTPS, true, 1);
        assert!(response_matches(
            &msg,
            0x1234,
            "cloudflare-ech.com",
            RR_HTTPS
        ));
    }

    #[test]
    fn rejects_a_spoofed_reply_with_the_wrong_transaction_id() {
        let msg = reply(0x9999, "cloudflare-ech.com", RR_HTTPS, true, 1);
        assert!(!response_matches(
            &msg,
            0x1234,
            "cloudflare-ech.com",
            RR_HTTPS
        ));
    }

    #[test]
    fn rejects_a_reply_for_a_different_name() {
        let msg = reply(0x1234, "attacker.example", RR_HTTPS, true, 1);
        assert!(!response_matches(
            &msg,
            0x1234,
            "cloudflare-ech.com",
            RR_HTTPS
        ));
    }

    #[test]
    fn rejects_a_reply_for_a_different_record_type() {
        let msg = reply(0x1234, "cloudflare-ech.com", 1, true, 1);
        assert!(!response_matches(
            &msg,
            0x1234,
            "cloudflare-ech.com",
            RR_HTTPS
        ));
    }

    #[test]
    fn rejects_a_message_that_is_not_a_response() {
        let msg = reply(0x1234, "cloudflare-ech.com", RR_HTTPS, false, 1);
        assert!(!response_matches(
            &msg,
            0x1234,
            "cloudflare-ech.com",
            RR_HTTPS
        ));
    }

    #[test]
    fn rejects_a_reply_with_an_unexpected_question_count() {
        let msg = reply(0x1234, "cloudflare-ech.com", RR_HTTPS, true, 2);
        assert!(!response_matches(
            &msg,
            0x1234,
            "cloudflare-ech.com",
            RR_HTTPS
        ));
    }

    #[test]
    fn rejects_truncated_input_without_panicking() {
        let msg = reply(0x1234, "cloudflare-ech.com", RR_HTTPS, true, 1);
        for cut in 0..msg.len() {
            assert!(!response_matches(
                &msg[..cut],
                0x1234,
                "cloudflare-ech.com",
                RR_HTTPS
            ));
        }
    }

    #[test]
    fn name_comparison_is_case_insensitive() {
        let msg = reply(0x1234, "CloudFlare-ECH.com", RR_HTTPS, true, 1);
        assert!(response_matches(
            &msg,
            0x1234,
            "cloudflare-ech.com",
            RR_HTTPS
        ));
    }
}
