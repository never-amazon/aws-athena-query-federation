fn main() -> Result<(), Box<dyn std::error::Error>> {
    let files = [
        "proto/substrait/algebra.proto",
        "proto/substrait/capabilities.proto",
        "proto/substrait/extended_expression.proto",
        "proto/substrait/extensions/extensions.proto",
        "proto/substrait/function.proto",
        "proto/substrait/parameterized_types.proto",
        "proto/substrait/plan.proto",
        "proto/substrait/type.proto",
        "proto/substrait/type_expressions.proto",
    ];
    for f in files {
        println!("cargo:rerun-if-changed={f}");
    }
    let fds = protox::compile(files, ["proto"])?;
    prost_build::Config::new().compile_fds(fds)?;
    Ok(())
}
