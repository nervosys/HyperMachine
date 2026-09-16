fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&["proto/vm.proto"], &["proto/"])?;

    // envd's real process.proto (e2b-dev/runtime), copied verbatim -- see
    // docs/CUBESANDBOX_PARITY_ROADMAP.md, Phase 1. A separate compile call:
    // it has its own package ("process"), unrelated to "hv2.v1" above.
    //
    // Server only: the proto's own `Connect` RPC collides with the
    // generated client's inherent `connect()` constructor (E0592) if the
    // client is built too. Nothing here needs a client for this service.
    tonic_prost_build::configure()
        .build_server(true)
        .build_client(false)
        .compile_protos(&["proto/process.proto"], &["proto/"])?;

    // envd's real filesystem.proto (e2b-dev/runtime), copied verbatim --
    // same source and reasoning as process.proto above. Server only, for
    // consistency (nothing here needs a client for this service either).
    //
    // Imports google/protobuf/timestamp.proto, which isn't in this repo --
    // it ships inside protoc's own release archive, under `include/`. Set
    // PROTOC_WELLKNOWN_INCLUDE to that directory (e.g. the `include/` next
    // to a manually-installed `protoc` binary) if compiling this fails with
    // "google/protobuf/timestamp.proto: File not found": a system protoc
    // installed via a package manager usually already knows where its own
    // well-known types live and needs no extra include path, but a protoc
    // binary fetched standalone (no package manager, no root) does not
    // carry that location anywhere runtime-discoverable.
    let mut fs_includes = vec!["proto/".to_string()];
    if let Ok(wellknown) = std::env::var("PROTOC_WELLKNOWN_INCLUDE") {
        fs_includes.push(wellknown);
    }
    tonic_prost_build::configure()
        .build_server(true)
        .build_client(false)
        .compile_protos(&["proto/filesystem.proto".to_string()], &fs_includes)?;

    Ok(())
}
