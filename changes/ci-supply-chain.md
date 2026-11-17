Security
CI checks the supply chain on every pull request: cargo-deny (RustSec advisories, licenses, crate sources), osv-scanner over the lockfiles and gitleaks over the history; Dependabot proposes weekly cargo and action updates. CI also builds the web client for wasm and checks Linux arm64; releases are built with the same Rust 1.99.1 as CI and carry a CycloneDX SBOM of each program.
