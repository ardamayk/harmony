//! Harmony Music Server in Rust. It replaces the Go server in
//! server/ module by module over the same SQLite database and HTTP
//! contract (packages/contracts/openapi.yaml); see ADR 0025.

pub mod api;
pub mod app;
pub mod auth;
pub mod config;
pub mod db;
pub mod dependencies;
pub mod gojson;
pub mod inspect;
pub mod migrate;
pub mod modules;
pub mod observability;
pub mod owner_access;
pub mod query;
pub mod respond;
pub mod searchdata;
pub mod static_assets;
