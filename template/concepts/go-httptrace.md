---
title: Go net/http/httptrace
summary: "Go per-request DNS/TCP/TLS/TTFB breakdown with stdlib httptrace and a TracingTransport wrapper"
type: concept
updated: 2026-11-17
sources:
  - sources/2026-06-01-go-httptrace.md
edges:
  - "EVIDENCED_BY 2026-06-01-go-httptrace"
---

# Go `net/http/httptrace` — client-side HTTP tracing

Stdlib (Go 1.7+). Surfaces what `http.Client` hides: DNS, TCP connect, TLS
handshake, connection acquired (reused? idle?), **first response byte**,
headers. No APM agent, no dependency.

## Why
Outbound latency is invisible until something is slow. Telling "DNS was slow"
from "the LLM took 8s to first byte" from "cold-pool TLS handshake" otherwise
means tcpdump. Here it's a 30-line patch. Most useful on services whose work is
outbound HTTP (LLM calls, webhooks, federation).

## The shape

```go
ctx := httptrace.WithClientTrace(req.Context(), &httptrace.ClientTrace{
    DNSStart:             func(httptrace.DNSStartInfo)      { /* ts */ },
    DNSDone:              func(httptrace.DNSDoneInfo)       { /* ts */ },
    ConnectStart:         func(network, addr string)        { /* ts */ },
    ConnectDone:          func(network, addr string, err error) { /* ts */ },
    TLSHandshakeStart:    func()                            { /* ts */ },
    TLSHandshakeDone:     func(tls.ConnectionState, error)  { /* ts */ },
    GotConn:              func(httptrace.GotConnInfo)       { /* Reused, WasIdle */ },
    GotFirstResponseByte: func()                            { /* ts */ },
})
req = req.WithContext(ctx)
```

Subtract timestamps → DNS / connect / TLS / TTFB / body: the five buckets `curl --trace-time` shows.

## Properties
- **Composes.** Multiple `WithClientTrace` on one context merge; middleware doesn't clobber caller hooks.
- **Zero overhead unused** — nil-check per hook. Safe in production paths.
- **Concurrency-safe** — the trace lives on the context, not the client.
- **`GotConnInfo.Reused` + `WasIdle`** — the only ergonomic way to see whether `MaxIdleConns*` does anything.

## Hooks that don't always fire
- **DNS** — skipped for IP hosts or resolver short-circuits.
- **TLS** — HTTPS only.
- **`RoundTrip` returns after headers.** For total duration incl. body, wrap `resp.Body` and record in `Close()`.

## Production shape: `TracingTransport`

```go
type TracingTransport struct {
    Base http.RoundTripper
    Log  func(req *http.Request, t *timings)
}
```

- `Base` stays the default `http.Transport` (pooling, HTTP/2 kept).
- `Log` routes to whatever the project uses — `slog`, JSON, Prometheus histograms.
- For LLM clients the two hooks that matter: `GotFirstResponseByte` (model vs
  network latency) and `GotConnInfo.Reused` (burning a TLS handshake per call?).

## Not for
- Server-side timing — middleware land.
- Retries / circuit breakers — httptrace measures, it doesn't decide.
- Plain wall-clock — `time.Since(start)` is enough.
- Microbenchmarks — that's in-process sampling ([[eval-methods]]); httptrace is the live-traffic side.

Back to the [[library]].
