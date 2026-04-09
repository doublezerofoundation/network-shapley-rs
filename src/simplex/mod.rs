//! Vendored solver internals from microlp v0.4.0.
//!
//! This module provides direct access to the simplex Solver, bypassing
//! microlp's `Problem` builder API to avoid per-constraint CsVec allocations.

#[allow(dead_code, clippy::all)]
pub(crate) mod helpers;
#[allow(dead_code, clippy::all)]
pub(crate) mod lu;
#[allow(dead_code, clippy::all)]
pub(crate) mod ordering;
#[allow(dead_code, clippy::all)]
pub(crate) mod solver;
#[allow(dead_code, clippy::all)]
pub(crate) mod sparse;
