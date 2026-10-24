# dusk_code

Rust workspace scaffold for an async harness core. The library defines domain boundaries; the CLI initializes logging, reports the Core API version, and exits.

```sh
cargo run -p dusk_code
cargo check --workspace
```

```text
projects/
├── core/                 dusk_core library
│   └── src/
│       ├── lib.rs        public modules
│       ├── api.rs        Core API version identity
│       ├── lifecycle.rs  lifecycle states
│       ├── events.rs     event contract and Tokio broadcast types
│       ├── extensions.rs extension metadata and registry storage
│       ├── policy.rs     async authorization contract
│       ├── persistence.rs async snapshot contract
│       └── supervision.rs cancellation and task ownership types
└── cli/                  dusk_code binary
    └── src/
        ├── main.rs       Tokio entry point
        ├── app.rs        build/run orchestration
        ├── args.rs       clap builder entry point
        ├── config.rs     logging setup
        └── constants.rs  application constants
```

Tokio provides the async runtime and concurrency primitives. Fallible contracts use `anyhow::Result`; policy and persistence futures are `Send`.

## Limitations

This is an initial scaffold. Lifecycle transitions, event routing, extension loading and compatibility negotiation, policy enforcement, persistence backends, and supervised shutdown are not implemented. The exposed traits and types mark their intended boundaries. The event transport is in-process Tokio broadcast and does not provide durable delivery.
