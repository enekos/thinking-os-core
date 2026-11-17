---
title: "Graph Engineering — codejunkie99/graph-engineering"
summary: "the source repo for the discipline: 9-stage KG pipeline, task-graph rules, /kg-* workflows → [[graph-engineering]]"
type: source
kind: repo
date: 2026-09-16
updated: 2026-11-17
---

# Graph Engineering (source)

`https://github.com/codejunkie99/graph-engineering` — MIT, by @Av1dlive. Ingested
2026-09-16 as the basis for refactoring this wiki. An English distillation of
东南大学《知识图谱》 (Southeast University's graduate Knowledge Graph course, Prof.
Peng Wang, `npubird/KnowledgeGraphCourse`) plus the agent-orchestration research
behind task graphs.

## The definition

> Graph engineering is the discipline of designing the structures AI agents work
> through — not the prompts. Prompt engineers steered the model's words. Loop
> engineers steered its iterations. Graph engineers steer its **topology**.

Two halves:

1. **Knowledge graphs** — what agents remember. Nodes are entities and facts,
   edges are relationships with time and provenance. Ontology → extraction →
   fusion → serving.
2. **Task graphs** — how agents work. Nodes are jobs, edges are execution
   dependencies. Parallel fan-out, separate verifiers, the stop rule, the human gate.

## The 9-stage pipeline

`scope → representation → ontology → entities → relations → events → quality gate
→ fusion → serve to LLMs`

Model the domain **before** extracting. Fuse **before** storing. Verify at every
stage. Stages 4-6 may collapse into one extraction pass on a small project, but
3 (ontology) and 8 (fusion) are never skipped — they are where real graphs fail.

Core mental model: **a knowledge graph is a product with a schema, not a pile of
triples.**

## Working rules quoted verbatim

- *Schema first, always.* Extraction without an ontology produces a "graph" that
  is really a word cloud with arrows.
- *Provenance on every fact.* Each node/edge stores `source`, `extracted_at` and
  confidence. Non-negotiable — fusion and trust both depend on it.
- *Incremental over big-bang.* A 10-document pilot through all 9 stages exposes
  ontology gaps at 1% of the cost.
- *LLM extraction is stage machinery, not the pipeline.* The LLM slots into
  stages 4-6; the surrounding schema, validation and fusion are what make the
  output a knowledge graph.
- Stage 7 target: **≥90% precision on a 50-item sample** before proceeding.
  Recall improves with more passes; bad precision poisons the graph permanently.
- Stage 3: every relation gets a precise verb name (`ACQUIRED`, not
  `RELATED_TO`); if two types are always queried together, merge them.

## The task-graph half

- **Fake edges.** For every "and then", ask whether the next job actually reads
  the previous job's output. If not, the edge is fake — delete it and the jobs
  run in parallel. Most hand-built pipelines contain two or three.
- **The diamond.** `plan → parallel workers → verify (separate context) → merge`.
  The verification node is non-negotiable: a model grading its own work in its
  own context misses most of its own mistakes. Give each verifier a different
  question — diverse skeptics catch what identical ones cannot.
- **The stop rule.** Google DeepMind × MIT, *Towards a Science of Scaling Agent
  Systems*, 180 controlled configurations: coordinated teams beat a single agent
  by ~80% on work that splits into independent pieces, and **every** multi-agent
  configuration lost on sequential work where each step needs the full picture
  (degrading 39-70%). Uncoordinated agents amplified each other's errors 17.2×;
  a single coordinator owning the merge cut it to 4.4×. *More agents is not a
  strategy. The shape of the work decides.*
- **The human gate.** The human is a node. Route every irreversible edge — send,
  publish, refund, delete, deploy — through explicit approval. Put the gate where
  a mistake is expensive to undo, not on every step. Judge the system on numbers
  that cannot argue back, never on its own self-reports.
- **Guardrails.** A round cap on every loop; one writer per file; the routing
  lives in written steps (the model fills the jobs, not the plan); a hard cap on
  spawned agents.

## Also in the repo

`WORKFLOWS.md` — nine paste-ready prompt blocks: `/kg-tutor` (teaches the whole
course interactively, one module per exchange, refusing to accept "makes sense"
as evidence of understanding) plus `/kg-scope`, `/kg-schema`, `/kg-extract`,
`/kg-relations`, `/kg-events`, `/kg-fuse`, `/kg-eval`, `/kg-rag` — each
consuming the previous one's output.

`graph-engineering/` — the packaged skill, with references on curriculum,
modeling, extraction, fusion+GraphRAG, and task graphs.
