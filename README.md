# OpenDisplay receiver

A `no_std` Rust library for receiving full-frame OpenDisplay transfers. It parses protocol messages, builds device configuration responses, reassembles direct-write and PIPE transfers, and validates and inflates compressed frames. It supports monochrome and four-gray frame geometry.

The application supplies the radio transport, board identity, transfer buffers, and display driver. The engine uses caller-owned buffers for staging, reordering, and decompression and performs no heap allocation. The application reports when it has displayed a frame so the engine can settle the transfer outcome.

Use `DisplayGeometry::new` for monochrome or `DisplayGeometry::with_format` for four-gray frames. Four-gray frames contain two sequential MSB-first bitplanes. The display adapter applies its panel's shade mapping. Advertise the transfer modes your adapter implements and use `config_response_frame_with_limit` when a negotiated BLE ATT payload is smaller than the default response size.

## Use in a Rust project

Add the receiver to your Cargo manifest. It requires Rust 1.95 or newer.

```toml
[dependencies]
opendisplay = { git = "https://github.com/ai212983/opendisplay-receiver.git", tag = "v0.1.1" }
```

The [crate documentation](src/lib.rs) shows how to initialize a receiver and get the required frame size.

## Test

```sh
cargo test --locked
```

Released under [MIT No Attribution](LICENSE). The compression dependency is [`miniz_oxide`](https://github.com/Frommi/miniz_oxide).
