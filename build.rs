fn main() {
    prost_build::compile_protos(&["src/proto/message.proto"], &["src/proto"])
        .expect("Failed to compile proto files");
}
