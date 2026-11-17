---
title: Eval-Driven Development — Lessons from Evaluating GenAI at Scale
summary: "Airbnb's eval-driven development: errors drive metrics, 3 evaluation layers, judge calibration → [[eval-methods]]"
type: source
kind: article
date: 2026-07-01
source_url: https://medium.com/airbnb-engineering/eval-driven-development-lessons-from-evaluating-genai-at-scale-e817e5ae5788
author: Rohit Girme (with Dan Miller, Mia Zhao, Lifan Yang, Clint Kelly) — Airbnb Tech Blog
published: 2026-07
ingested: 2026-08-04
---

# Eval-Driven Development: Lessons from Evaluating GenAI at Scale

## Introduction

Generative AI fundamentally differs from traditional software testing. LLM outputs are non-deterministic, and determining correctness involves subjective judgment. At Airbnb, teams build AI-powered features across multiple products while facing unique evaluation challenges.

## 1. Foundation

### 1.1 The One Rule

The starting point for any evaluation is straightforward: "When in doubt, look at your data." Teams should manually review outputs from roughly 100 examples, categorize mistakes, and build evaluations around observed failures rather than theoretical ones.

### 1.2 Eval-Driven Development (EDD)

EDD represents the GenAI equivalent of test-driven development, emphasizing continuous discovery and testing of failure modes. Five core principles anchor this approach:

1. Define goals and success criteria upfront
2. Let real errors guide metric development
3. Maintain focused, well-calibrated evaluators (3–5 rather than 20–30)
4. Designate a human decision-maker for disagreements
5. Maintain cross-functional collaboration throughout

## 2. Three Evaluation Methods

### 2.1 Programmatic & Heuristic Metrics

Deterministic, code-based checks form the first layer, catching obvious failures before more resource-intensive evaluation. Use structured outputs (JSON schemas) to ensure strict typing rather than relying on prompt instructions alone.

### 2.2 LLM-as-Judge (Virtual Judges)

Stronger LLMs evaluate other LLMs' outputs against carefully designed rubrics, assessing nuanced qualities like tone and coherence. Effective rubrics eliminate ambiguity and include specific scoring criteria with clear examples.

#### 2.2.1 Calibration

Virtual judges require calibration using a golden dataset of 50–100 examples (including failures):

1. Create a labeled golden set
2. Run the judge against it
3. Measure agreement (target high 80s–90s%)
4. Analyze and resolve disagreements
5. Recalibrate periodically

### 2.3 Human Evaluation

Human judgment remains essential for ground truth, high-stakes decisions, and resolving disagreements between automated systems.

### 2.4 Evaluation Scenarios

Start with 20–100 expert-labeled examples before scaling to larger annotation workforces. If human experts disagree, resolution must occur before automation.

## 3. Evaluating Agentic Systems

Multi-step agentic systems require evaluation across three layers:

- **Agent trajectory** (reasoning path quality)
- **Tool usage** (correct tools with appropriate parameters)
- **Final output** (result correctness)

Evaluation only of final outputs masks broken reasoning or inefficient paths. Teams can leverage application traces and tree traversal to reconstruct and evaluate complete agent behavior.

## 4. Practical Walkthrough

**Step 1: Exploration** — Run 100 examples, categorize failure modes
**Step 2: Build Evals** — Add programmatic checks and virtual judges
**Step 3: Calibrate** — Achieve 80s–90s% agreement with experts
**Step 4: Scale & Monitor** — Deploy across larger datasets with production monitoring

## Key Takeaways

- Examine data before building evaluation frameworks
- Develop evaluators targeting specific product failure modes, not generic metrics
- Start small (50–100 examples) and iterate quickly
- Assign one evaluator per dimension
- Achieve high 80s–90s% calibration agreement before scaling
- Include negative examples in golden datasets
- Evaluate complete system pipelines, not isolated components
- Mirror evaluation approaches in production environments
- Treat evaluation as a collaborative, cross-functional discipline
