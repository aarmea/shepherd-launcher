# Internet Connection Gating

Issue: <https://github.com/aarmea/shepherd-launcher/issues/9>

Summary:
- Added internet connectivity configuration in the shepherdd config schema and policy, with global checks and per-entry requirements.
- Implemented a connectivity monitor in shepherdd and enforced internet-required gating in the core engine.
- Added a new ReasonCode for internet-unavailable, updated launcher UI message mapping, and refreshed docs/examples.

Key files:
- crates/shepherd-config/src/schema.rs
- crates/shepherd-config/src/policy.rs
- crates/shepherd-config/src/internet.rs
- crates/shepherd-config/src/validation.rs
- crates/shepherd-core/src/engine.rs
- crates/shepherdd/src/internet.rs
- crates/shepherdd/src/main.rs
- crates/shepherd-api/src/types.rs
- crates/shepherd-launcher-ui/src/client.rs
- config.example.toml
- crates/shepherd-config/README.md
