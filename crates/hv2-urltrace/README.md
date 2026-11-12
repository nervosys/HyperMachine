# hv2-urltrace

`hv2-urltrace` fetches one URL, once, and prints one JSON report. It is meant to be the only program in a minimal, throwaway Linux guest whose job is to look at a URL that may be malicious.

```
hv2-urltrace <URL> [--max-body BYTES] [--timeout SECS] [--user-agent STRING] [--resolve IPV4]
```

## The one-hop rule

The tool never follows a redirect. If the response is a redirect, the report says where it points (`location`, and `next_url` when that is a usable http or https URL) and the program exits. The host reads the report, destroys the VM, and starts a fresh VM for the next hop. A redirect chain therefore never shares a guest, so a hop that compromises the guest cannot see or influence the hops after it, and each hop starts from a clean image.

## Behaviour

1. Parse the URL (http or https; DNS name or IPv4 literal; optional port; no userinfo, no IPv6; at most 8192 bytes).
2. Resolve with the standard library, IPv4 only, report up to 16 addresses, connect to the first that accepts. `--resolve IP` skips DNS; the SNI and `Host` header still use the URL's host.
3. For https, a rustls handshake offering only `http/1.1`. The certificate is validated for real against the `webpki-roots` trust store and the verdict is recorded, but the verdict does not stop the fetch, because an analyst needs the response from a host with a bad certificate. Signature checks are delegated to the real verifier.
4. Send `GET`, with `Host`, `User-Agent`, `Accept: */*`, `Accept-Encoding: identity` and `Connection: close`, and nothing else.
5. Read the response with a bounded HTTP/1.1 parser (Content-Length, chunked, or read-to-close).
6. Scan the bytes read for redirect hints that are not HTTP redirects.

Every stage that fails records `{"stage": ..., "message": ...}` in `errors` and the report still carries whatever was gathered. The exit status is 0 whenever a report was printed; it is non-zero (2) only for bad arguments. Diagnostics go to stderr; stdout carries the JSON object and nothing else.

## Report fields

- `tool`: `name`, `version`.
- `input_url`, `url` (`scheme`, `host`, `port`, `path_and_query`).
- `resolved`: IPv4 `addr:port` strings (at most 16); `connected_to`.
- `tls`: `protocol_version`, `cipher_suite`, `alpn`, `verified` (true, false, or null if no certificate was seen), `verification_error`, and `certificates` (at most 8) each with `sha256` of the DER, `subject`, `issuer`, `serial` (hex), `not_before` and `not_after` (unix seconds), `san_dns` (at most 64), `san_dns_truncated`, `parse_error`.
- `request`: the exact `request_line` and `headers` sent.
- `response`: `http_version`, `status`, `reason`, `headers` (ordered `[name, value]` pairs, names as sent), `headers_truncated`, `lossy` (true if any header or reason bytes were not UTF-8 and were decoded lossily), `location` (raw), `next_url`, `content_type`, `body_bytes_read`, `body_truncated`, `body_sha256` (of the bytes read), `body_sample_encoding` and `body_sample` (first 2048 bytes, base64).
- `hints`: `{kind, value}` entries from a bounded byte scan: `meta_refresh` (an HTML `<meta http-equiv="refresh">`), and up to 5 of `script_window_location`, `script_location_replace`, `script_location_href` with up to 200 bytes of following context. Nothing is parsed or executed; these are hints for a human.
- `timings_ms`: `resolve`, `connect`, `tls` (each the duration of that stage), `first_byte` (from sending the request to the first response byte), `total`.
- `errors`, `notes`. `notes` explain, for instance, why `next_url` is null.

All strings in the report are length-capped; a capped string ends in `...[truncated]`.

## Limits

Defaults: 262144 body bytes (hard cap 4 MiB whatever is asked), a 15 second timeout applied to resolve, connect, handshake and each read, and also as an overall deadline (capped at 120 s). Headers: at most 64 KiB and 200 lines; chunk-size lines at most 128 bytes.

It fetches and does not render. There is no JavaScript, no HTTP/2, no IPv6, no cookies, no proxy support, no compression (`Accept-Encoding: identity`), and no reading of environment variables, configuration files, or the filesystem. Name resolution has no cancellable timeout in the standard library, so it runs on a helper thread that is abandoned if the deadline passes. The code is `#![forbid(unsafe_code)]`.

## Do not trust the report by itself

The guest that produced the report is assumed to be compromised: the hostile server's bytes were parsed inside it. The report is therefore a claim, not evidence. The host's own egress log (what the guest actually connected to, and how much it sent and received) is the independent record, and anything that matters should be checked against it.
