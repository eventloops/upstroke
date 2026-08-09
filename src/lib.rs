//! tactus — headless orchestration engine for AI coding agents.
//!
//! Copyright (C) 2026 Cameron Lambert
//!
//! This program is free software: you can redistribute it and/or modify it
//! under the terms of the GNU Affero General Public License as published by
//! the Free Software Foundation, either version 3 of the License, or (at your
//! option) any later version. It is distributed WITHOUT ANY WARRANTY; without
//! even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR
//! PURPOSE. See the GNU Affero General Public License for more details. You
//! should have received a copy of the License along with this program; if
//! not, see <https://www.gnu.org/licenses/>.
//!
//! Commercial licences are available for use that the AGPL does not permit —
//! see README.md.
//!
//! Step 1 scope: `tactus validate` only. Parse an annotated markdown plan
//! into the IR, load optional config, resolve routing chains with a binder
//! preview, and report — executing nothing.

pub mod agent;
pub mod catalog;
pub mod config;
pub mod engine;
pub mod error;
pub mod gates;
pub mod ir;
pub mod plan;
pub mod review;
pub mod route;
pub mod ulid;
pub mod util;
pub mod validate;
pub mod workspace;
