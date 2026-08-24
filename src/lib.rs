// SPDX-License-Identifier: Apache-2.0
//! Library entry point for dgmon.
//!
//! This exposes the modules needed by integration tests. The binary
//! (`main.rs`) keeps its own module tree; this library mirrors the
//! relevant modules so tests can use the public API.

pub mod collector;
pub mod metric_name;
pub mod storage;
