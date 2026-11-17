---
title: "Tracing HTTP Requests with Go's net/http/httptrace"
summary: "httptrace walkthrough by Blain Smith → [[go-httptrace]]"
type: source
kind: article
date: 2026-05-26
url: https://blainsmith.com/articles/httptrace-with-go/
author: Blain Smith
updated: 2026-11-17
---

# Tracing HTTP Requests with Go's net/http/httptrace

**By Blain Smith | May 26, 2026**

## Overview

The `net/http/httptrace` package, available since Go 1.7, provides visibility into HTTP request lifecycle events typically hidden from developers: DNS resolution, connection acquisition, TLS handshakes, and response timing. As the author notes, "most Go developers I talk to have never used it," despite its utility for debugging slow requests.

## Design Philosophy: Context Over Interfaces

Rather than implementing a traditional `Tracer` interface on `http.Client`, Go's approach uses context propagation. Developers attach a `ClientTrace` to a context via `httptrace.WithClientTrace()`, then associate that context with their request. The transport retrieves it internally when needed.

This design offers several advantages:

- **Composability**: Traces travel with requests, propagating automatically through middleware that forwards context
- **Concurrency-safe**: Different concurrent requests can carry different traces without shared mutable state
- **Zero overhead**: Unused traces cost only a nil check
- **Forward compatible**: New hooks can be added without breaking existing code

The `ClientTrace` struct contains optional function fields like `DNSStart`, `DNSDone`, `TLSHandshakeStart`, `GotFirstResponseByte`, and others — you only populate the ones you need.

## Practical Implementation: A Timing CLI

The article demonstrates building a command-line tool resembling `curl --trace`. The implementation captures timestamps at each trace hook, then computes durations:

```go
type timings struct {
    start        time.Time
    dnsStart     time.Time
    dnsDone      time.Time
    connectStart time.Time
    connectDone  time.Time
    tlsStart     time.Time
    tlsDone      time.Time
    gotConn      time.Time
    firstByte    time.Time
    done         time.Time
}
```

The trace captures these moments, allowing calculation of breakdowns like DNS lookup duration, TCP connection time, TLS handshake duration, server processing time, and content transfer time.

## Building a RoundTripper

For ongoing request tracing across an `http.Client`, wrapping the transport provides automatic logging:

```go
type TracingTransport struct {
    Base http.RoundTripper
    Log  func(req *http.Request, t *timings)
}

func (tt *TracingTransport) RoundTrip(req *http.Request) (*http.Response, error) {
    // Attach trace to request context, execute, log results
}
```

An important nuance: calling `httptrace.WithClientTrace` multiple times composes traces rather than replacing them, so caller-attached traces are preserved.

## Connection Reuse Insights

The `GotConnInfo` struct provides `Reused` and `WasIdle` boolean fields, revealing whether the client successfully pooled connections. This information typically requires packet capture to verify — "This is the kind of thing you would otherwise need a packet capture to verify."

## Practical Limitations

Some hooks only fire under specific conditions:

- DNS hooks only fire if Go's resolver performs the lookup (not for cached/direct IP addresses)
- TLS hooks only fire on HTTPS
- `RoundTrip` returns after headers are read, not after body consumption

For total request duration including body transfer, wrap the response body to record completion time.

## Conclusion

The package provides a lightweight, composable solution for HTTP request tracing without external dependencies or APM agents, making it suitable for debugging performance issues in standalone applications.
