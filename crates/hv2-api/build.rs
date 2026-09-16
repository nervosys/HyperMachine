/// Generate protobuf-JSON (serde) impls for one package's messages.
///
/// Separate from the tonic compile because the two want different things: the
/// tonic build produces the service traits and the prost structs, and this
/// adds the JSON mapping on top of those same structs, from the descriptor set
/// the tonic build was asked to leave behind.
///
/// Needed at all because E2B's SDK speaks the Connect protocol with a JSON
/// codec, and protobuf-JSON is a specified mapping rather than whatever serde
/// would derive: lowerCamelCase fields, 64-bit integers as strings, enums by
/// name, `Timestamp` as RFC 3339. Getting that wrong is not a compile error --
/// it is a client that silently reads zero from every `size` field.
fn json_for(descriptor: &std::path::Path, package: &str) -> Result<(), Box<dyn std::error::Error>> {
    pbjson_build::Builder::new()
        .register_descriptors(&std::fs::read(descriptor)?)?
        .build(&[package])?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
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
    let process_descriptor = out_dir.join("process_descriptor.bin");
    tonic_prost_build::configure()
        .build_server(true)
        .build_client(false)
        .file_descriptor_set_path(&process_descriptor)
        .compile_protos(&["proto/process.proto"], &["proto/"])?;
    json_for(&process_descriptor, ".process")?;

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
    let fs_descriptor = out_dir.join("filesystem_descriptor.bin");
    tonic_prost_build::configure()
        .build_server(true)
        .build_client(false)
        // `google.protobuf.Timestamp` comes from `pbjson-types` rather than
        // `prost-types`: the two are the same struct, but only the former
        // carries protobuf-JSON's RFC 3339 mapping, and pbjson's generated
        // code for `EntryInfo` needs to serialise the field.
        //
        // `compile_well_known_types` must be on for the `extern_path` to be
        // accepted at all: without it the builder has already mapped
        // `.google.protobuf` to prost-types itself, and adding a second
        // mapping fails with "duplicate extern Protobuf path".
        .compile_well_known_types(true)
        .extern_path(".google.protobuf", "::pbjson_types")
        .file_descriptor_set_path(&fs_descriptor)
        .compile_protos(&["proto/filesystem.proto".to_string()], &fs_includes)?;
    json_for(&fs_descriptor, ".filesystem")?;

    Ok(())
}
