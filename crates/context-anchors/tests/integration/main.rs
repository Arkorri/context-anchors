//! The one integration binary: every module drives the real `anchr` executable against fixture
//! repositories built in temp dirs. One binary rather than one crate per file so the modules share
//! `support::Fixture` and link the dev-dependencies once. Tests share a process, so nothing here
//! may touch global state; every test builds its own fixture.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

mod cli;
mod coverage;
mod init;
mod lsp;
mod tools;
