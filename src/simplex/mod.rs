//! Vendored solver internals from microlp v0.4.0.
//!
//! This module provides direct access to the simplex [`solver::Solver`], bypassing
//! microlp's `Problem` builder API to avoid per-constraint CsVec allocations.
//!
//! ## How the pieces fit together
//!
//! ```text
//! shapley.rs  ──▶  solver.rs (solve_coalition)
//!                       │
//!                       ▼
//!               simplex::solver::Solver   ← the LP engine
//!                   ├── simplex::lu        ← LU factorisation of the basis matrix
//!                   ├── simplex::sparse    ← sparse vector / matrix types used internally
//!                   ├── simplex::ordering  ← column pre-ordering heuristic for LU
//!                   └── simplex::helpers   ← small sparse-vector utilities
//! ```
//!
//! The `#[allow(dead_code)]` annotations exist because this is a vendored library
//! with internal infrastructure that isn't all reachable from our public API, but
//! is required for the solver's correctness.

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
