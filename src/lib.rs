//! upstroke — headless orchestration engine for AI coding agents.
//!
//! Copyright 2026 Cameron Lambert
//! SPDX-License-Identifier: Apache-2.0
//!
//! Licensed under the Apache License, Version 2.0. Distributed on an "AS IS"
//! basis, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND; see the LICENSE and
//! NOTICE files at the repository root, or
//! <http://www.apache.org/licenses/LICENSE-2.0>.
//!
//! v0.1 scope (DESIGN.md §21, steps 1–10): parse an annotated markdown plan
//! into the IR, resolve a routing chain per task, and execute it sequentially —
//! one agent subprocess per attempt, gates and read-only review over the
//! engine-captured diff, one commit per task, every transition an event in
//! `events.jsonl`. `resume`, `status`, and `answer` are folds over that log.
//!
//! The capacity engine (§13) ships **read-only**: `connect` discovers the agent
//! CLIs and writes the pools file, `capacity` and the dry-run preview estimate
//! what is left and what each strategy *would* do, and budgets stop a run at a
//! ceiling — but nothing routes on any of it. Capacity-driven binding is v0.2.

pub mod agent;
pub mod answer;
pub mod capacity;
pub mod catalog;
pub mod config;
pub mod connect;
pub mod effects;
pub mod engine;
pub mod error;
pub mod events;
pub mod export;
pub mod gates;
pub mod interaction;
pub mod ir;
pub mod ladder;
pub mod plan;
pub mod review;
pub mod route;
pub mod rundir;
pub mod runner;
pub mod status;
pub mod topology;
pub mod ulid;
pub mod util;
pub mod validate;
pub mod workspace;
pub mod workspace_manager;
