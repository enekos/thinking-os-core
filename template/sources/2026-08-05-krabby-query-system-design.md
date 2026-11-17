---
title: "Designing a Query System"
summary: "Arya Dradjica's pull-based compiler query system design → [[query-system-design]]"
type: source
kind: article
date: 2026-08-03
url: https://bal-e.org/speed/krabby/2026/query-system-design/
author: Arya Dradjica
license: CC BY-NC-SA 4.0
updated: 2026-11-17
---

# Designing a Query System

**By:** Arya Dradjica
**Date:** August 3, 2026

---

## Overview

This article presents a comprehensive design for Krabby's query system—a compiler infrastructure that moves away from push-based task execution toward pull-based on-demand computation. The author spent five months developing this design to support concurrent compilation while maintaining flexibility for future optimizations.

## Key Motivation

The original push-based architecture had critical limitations. As the author explains, it functioned as a "specialized query system" despite avoiding explicit query mechanics, couldn't efficiently use CPU caches, and struggled with unbalanced dependency graphs. A pull-based approach enables demand-driven prioritization essential for LSP integration and different compilation goals.

## Desired Features

The design targets eight major capabilities:

**Concurrency**: Multi-threaded task distribution with contention handling and cross-thread cycle detection, treating parallelism as a requirement from day one.

**Asynchrony**: Tasks pause and resume rather than blocking threads. This allows concurrent dependency resolution, batched multi-query operations, and async I/O operations like network fetches.

**Batching**: Processing 64 similar tasks simultaneously improves cache utilization and enables future SIMD optimizations for compiler stages beyond lexing.

**Pre-emptive execution**: Idle worker threads eagerly find new tasks using prioritization heuristics based on critical path analysis.

**Cargo integration**: Compiling entire dependency graphs simultaneously, starting work on dependent crates before their dependencies finish, and communicating via shared memory rather than disk files.

**Better incrementality**: Context-free caching with configurable eviction policies (LRU) allows limiting target folder sizes and reusing caches across feature flag changes and Git commits.

**Streaming queries**: Collections return results as discovered rather than waiting for complete enumeration, reducing latency in operations like "find all references."

**Accelerated testing**: Detecting code changes and selectively re-running affected compiler tests using the query system's incremental mechanisms.

## System Architecture

### Core Components

**Tasks** are stateful computations holding references to output slots. Each task has:
- A `poll()` function containing execution logic (similar to Rust futures)
- Optional batch `poll()` implementations for efficiency
- Priority scores reflecting critical path position
- Slot metadata tracking execution state

**Queries** link tasks as dependencies. They transition from initiated → pending/complete → possibly blocked if cross-thread dependencies exist. The system supports streaming queries returning incremental results.

**Worker threads** execute tasks in batches by class, maintaining local ready-to-execute sets. They track task hierarchies and parent-child relationships for proper resumption.

**Caching** operates on two levels:
- In-memory: Essential runtime cache containing slots and task metadata
- On-disk: Incremental compilation cache storing task recordings with canonicalized, interned values for deduplication

### Sophisticated Features

The design includes streaming query support for collections discovered over time, allowing dependent tasks to observe partial results while blocked.

Incremental compilation extends the red-green algorithm across multiple prior compilation sessions. Tasks check the current and previous caches sequentially; missing prior data doesn't prevent eventual recomputation—older cache entries may still provide value.

Cycle detection employs three algorithms: a fast per-thread eager check, a lazy cross-thread approach using reachability analysis, and a slow confirmation that walks the dependency graph. This handles cycles in concurrent, batched execution contexts where traditional approaches fail.

## Implementation Status

The author plans to present implementations at EuroRust 2026, including comparative versions using Salsa and the new Krabby query system. Development of the query infrastructure is beginning immediately.

---

**License**: CC BY-NC-SA 4.0
